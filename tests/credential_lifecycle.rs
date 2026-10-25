//! Real pass/GPG integration using an ephemeral synthetic key and fake API credential.
#![cfg(unix)]
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

const FAKE_KEY: &str = "clisage-test-secret-never-real";

struct Store {
    root: tempfile::TempDir,
}
impl Store {
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", self.root.path())
            .env("GNUPGHOME", self.root.path().join("gnupg"))
            .env("PASSWORD_STORE_DIR", self.root.path().join("store"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env_remove("GPG_AGENT_INFO")
            .env_remove("GPG_TTY")
            .env_remove("PASSWORD_STORE_KEY")
            .env_remove("PASSWORD_STORE_GPG_OPTS")
            .env_remove("PASSWORD_STORE_GPG")
            .env_remove("PASSWORD_STORE_EXTENSIONS_DIR")
            .env("PASSWORD_STORE_ENABLE_EXTENSIONS", "false")
            .env_remove("CLISAGE_API_KEY")
            .stdin(Stdio::null());
        command
    }
    fn checked(&self, program: &str, args: &[&str]) -> Vec<u8> {
        let output = self.command(program).args(args).output().unwrap();
        assert!(output.status.success(), "isolated {program} failed");
        output.stdout
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self
            .command("gpgconf")
            .args([
                "--homedir",
                self.root.path().join("gnupg").to_str().unwrap(),
                "--kill",
                "gpg-agent",
            ])
            .output();
    }
}
struct ChildCleanup(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for ChildCleanup {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_until(rx: &mpsc::Receiver<Vec<u8>>, transcript: &mut Vec<u8>, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !String::from_utf8_lossy(transcript).contains(needle) {
        transcript.extend(
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("credential lifecycle terminal timed out"),
        );
    }
}

#[test]
fn login_encrypts_reads_back_and_logout_removes_only_the_scoped_credential() {
    let store = Store {
        root: tempfile::tempdir().unwrap(),
    };
    let home = store.root.path().join("gnupg");
    std::fs::create_dir(&home).unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    store.checked(
        "gpg",
        &[
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "",
            "--quick-generate-key",
            "Clisage Test <fixture@example.invalid>",
            "rsa2048",
            "encrypt",
            "0",
        ],
    );
    let keys =
        String::from_utf8(store.checked("gpg", &["--batch", "--with-colons", "--list-keys"]))
            .unwrap();
    let recipient = keys
        .lines()
        .find_map(|line| {
            let fields: Vec<_> = line.split(':').collect();
            (fields.first() == Some(&"fpr")).then(|| fields[9])
        })
        .unwrap();
    store.checked("pass", &["init", "-p", "clisage", recipient]);
    let unrelated = store.root.path().join("store/unrelated.txt");
    std::fs::write(&unrelated, "non-secret fixture data").unwrap();

    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_clisage"));
    command.args(["auth", "login"]);
    command.env("HOME", store.root.path());
    command.env("GNUPGHOME", &home);
    command.env("PASSWORD_STORE_DIR", store.root.path().join("store"));
    command.env("XDG_CONFIG_HOME", store.root.path().join("config"));
    for key in [
        "CLISAGE_API_KEY",
        "GPG_AGENT_INFO",
        "GPG_TTY",
        "PASSWORD_STORE_KEY",
        "PASSWORD_STORE_GPG_OPTS",
        "PASSWORD_STORE_GPG",
        "PASSWORD_STORE_EXTENSIONS_DIR",
    ] {
        command.env_remove(key);
    }
    command.env("PASSWORD_STORE_ENABLE_EXTENSIONS", "false");
    let mut child = ChildCleanup(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut transcript = Vec::new();
    read_until(&rx, &mut transcript, "OpenAI API key:");
    let deadline = Instant::now() + Duration::from_secs(3);
    while pair.master.get_termios().unwrap().local_flags.bits() & libc::ECHO != 0 {
        assert!(
            Instant::now() < deadline,
            "password prompt did not disable terminal echo"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    writeln!(writer, "{FAKE_KEY}").unwrap();
    read_until(
        &rx,
        &mut transcript,
        "Credential stored and read back successfully.",
    );
    assert!(child.0.wait().unwrap().success());
    assert!(!String::from_utf8_lossy(&transcript).contains(FAKE_KEY));

    let encrypted_path = store
        .root
        .path()
        .join("store")
        .join(format!("{}.gpg", clisage::CREDENTIAL_ID));
    let ciphertext = std::fs::read(&encrypted_path).unwrap();
    assert!(
        !ciphertext
            .windows(FAKE_KEY.len())
            .any(|bytes| bytes == FAKE_KEY.as_bytes())
    );
    let decrypted = store.checked("pass", &["show", clisage::CREDENTIAL_ID]);
    assert_eq!(String::from_utf8(decrypted).unwrap().trim_end(), FAKE_KEY);
    let status = store.checked(env!("CARGO_BIN_EXE_clisage"), &["auth", "status"]);
    assert!(!String::from_utf8_lossy(&status).contains(FAKE_KEY));
    store.checked(env!("CARGO_BIN_EXE_clisage"), &["auth", "logout"]);
    assert!(!encrypted_path.exists());
    assert!(Path::new(&unrelated).exists());
}

#[cfg(unix)]
#[test]
fn interactive_login_checks_store_before_requesting_api_key() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::Read,
        sync::mpsc,
        time::{Duration, Instant},
    };

    struct Cleanup(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let pair = native_pty_system().openpty(PtySize::default()).unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_clisage"));
    command.args(["auth", "login"]);
    command.env("HOME", dir.path());
    command.env("XDG_CONFIG_HOME", dir.path());
    command.env("PASSWORD_STORE_DIR", dir.path().join("store"));
    command.env_remove("PASSWORD_STORE_KEY");
    command.env_remove("CLISAGE_API_KEY");
    let mut child = Cleanup(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut data = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&data);
        assert!(
            !text.contains("OpenAI API key:"),
            "asked for a secret before checking storage"
        );
        if text.contains("pass init -p clisage YOUR_GPG_KEY_ID") {
            break;
        }
        data.extend(
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("missing store initialization guidance: {text}")),
        );
    }
    assert!(!child.0.wait().unwrap().success());
    assert!(!dir.path().join("store").exists());
}

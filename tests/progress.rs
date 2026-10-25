use clisage::ThinkingIndicator;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone)]
struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    changed: mpsc::Sender<()>,
}
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        let _ = self.changed.send(());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn thinking_animates_and_clears_before_stop_returns() {
    let (changed, rx) = mpsc::channel();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut indicator = ThinkingIndicator::start(
        true,
        None,
        Capture {
            bytes: bytes.clone(),
            changed,
        },
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        if output.contains("⠋") && output.contains("⠙") {
            break;
        }
        rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .expect("spinner did not animate");
    }
    indicator.stop();
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(output.contains(""));
    assert!(output.contains("Clisage: thinking..."));
    assert!(output.ends_with("\r\x1b[2K"));
    let after_stop = bytes.lock().unwrap().len();
    indicator.stop();
    drop(indicator);
    assert_eq!(bytes.lock().unwrap().len(), after_stop);
}

#[test]
fn disabled_indicator_emits_nothing_and_custom_icon_cannot_inject_controls() {
    for enabled in [false, true] {
        let (changed, rx) = mpsc::channel();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let indicator = ThinkingIndicator::start(
            enabled,
            Some("CUSTOM\n\x1b\u{202e}"),
            Capture {
                bytes: bytes.clone(),
                changed,
            },
        );
        if enabled {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        drop(indicator);
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        if enabled {
            assert!(output.contains("CUSTOM"));
            assert!(!output.contains("CUSTOM\n"));
            assert!(!output.contains('\u{202e}'));
            assert!(output.ends_with("\r\x1b[2K"));
        } else {
            assert!(output.is_empty());
        }
    }
}

#[test]
fn first_streamed_text_stops_progress_before_writing_the_response() {
    use clisage::ThinkingOutput;
    let (changed, rx) = mpsc::channel();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut indicator = ThinkingIndicator::start(
        true,
        None,
        Capture {
            bytes: bytes.clone(),
            changed,
        },
    );
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut response = Vec::new();
    let mut output = ThinkingOutput::new(&mut response, &mut indicator, true);
    output.write_all(b"answer").unwrap();
    assert!(String::from_utf8_lossy(&bytes.lock().unwrap()).ends_with("\r\x1b[2K"));
    output.write_all(b" continues").unwrap();
    assert_eq!(response, b"answer continues");
}

#[cfg(unix)]
#[test]
fn credential_unlock_prompts_are_not_overwritten_by_the_spinner() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{io::Read, os::unix::fs::PermissionsExt};
    struct Cleanup(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for term in ["xterm", "dumb"] {
        let dir = tempfile::tempdir().unwrap();
        let pass = dir.path().join("pass");
        std::fs::write(
            &pass,
            "#!/bin/sh\nprintf 'TEST PINENTRY active\\n' > /dev/tty\n/bin/sleep 0.4\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&pass, std::fs::Permissions::from_mode(0o700)).unwrap();
        let pair = native_pty_system().openpty(PtySize::default()).unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_clisage"));
        command.args(["shell", "--command-only", "fake request"]);
        command.env("PATH", dir.path());
        command.env("HOME", dir.path());
        command.env("XDG_CONFIG_HOME", dir.path());
        command.env("PASSWORD_STORE_DIR", dir.path().join("store"));
        command.env_remove("PASSWORD_STORE_KEY");
        command.env_remove("CLISAGE_API_KEY");
        command.env("CLISAGE_THINKING_ICON", "TESTICON");
        command.env("TERM", term);
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
        let mut bytes = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&bytes).contains("pass init -p clisage YOUR_GPG_KEY_ID") {
            bytes.extend(
                rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .expect("CLI did not finish authentication failure"),
            );
        }
        assert!(!child.0.wait().unwrap().success());
        let output = String::from_utf8(bytes).unwrap();
        assert!(output.contains("TEST PINENTRY active"));
        assert!(!output.contains("Clisage: thinking"));
        assert!(!output.contains("\x1b[2K"));
    }
}

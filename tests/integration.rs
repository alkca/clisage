//! Exercise the generated integration with real shells and a fake CLI.
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn integration(shell: &str) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_clisage"))
        .args(["integration", shell])
        .env_remove("CLISAGE_API_KEY")
        .env("XDG_CONFIG_HOME", dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert!(!dir.path().join("clisage").exists());
    result.stdout
}

#[test]
fn prints_integration_without_credentials_and_rejects_unsupported_shells() {
    assert!(
        String::from_utf8(integration("zsh"))
            .unwrap()
            .contains("bindkey")
    );
    assert!(
        String::from_utf8(integration("bash"))
            .unwrap()
            .contains("-x")
    );
    let out = Command::new(env!("CARGO_BIN_EXE_clisage"))
        .args(["integration", "fish"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn integration_output_is_independent_of_config_and_credentials() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("clisage")).unwrap();
    std::fs::write(dir.path().join("clisage/config.toml"), "not valid toml").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_clisage"))
        .args(["integration", "zsh"])
        .env("XDG_CONFIG_HOME", dir.path())
        .env("CLISAGE_API_KEY", "fake-secret\ninvalid")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-secret"));
}

#[cfg(unix)]
fn widget(shell: &str, original: &str, suggestion: &str, status: &str) -> (String, String) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("integration");
    std::fs::write(&script, integration(shell)).unwrap();
    let executable = dir.path().join("clisage");
    std::fs::write(&executable, r#"#!/bin/sh
[ "$1" = shell ] && [ "$2" = --command-only ] && [ "$3" = -- ] && [ "$4" = "$CLISAGE_TEST_PROMPT" ] && [ "$#" = 4 ] || exit 90
[ "$CLISAGE_SHELL" = "$CLISAGE_TEST_SHELL" ] || exit 93
# The widget must never feed terminal input to the CLI.
if IFS= read -r ignored; then exit 91; fi
printf '%s' "$CLISAGE_TEST_OUTPUT"
exit "$CLISAGE_TEST_STATUS"
"#).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let harness = match shell {
        "zsh" => {
            r#"
            zle() { :; }
            source "$CLISAGE_TEST_SCRIPT"
            BUFFER=$CLISAGE_TEST_PROMPT
            CURSOR=2
            _clisage_zsh_widget
            printf '%s\n%s' "$BUFFER" "$CURSOR"
        "#
        }
        "bash" => {
            r#"
            source "$CLISAGE_TEST_SCRIPT"
            READLINE_LINE=$CLISAGE_TEST_PROMPT
            READLINE_POINT=2
            _clisage_bash_widget
            printf '%s\n%s' "$READLINE_LINE" "$READLINE_POINT"
        "#
        }
        _ => unreachable!(),
    };
    let shell_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join(shell))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("install {shell} to run shell integration tests"));
    let mut cmd = Command::new(shell_path);
    if shell == "zsh" {
        cmd.args(["-dfc", harness]);
    } else {
        cmd.args(["--noprofile", "--norc", "-c", harness]);
    }
    let marker = dir.path().join("must-not-execute");
    let mut child = cmd
        .env("PATH", dir.path())
        .env("CLISAGE_TEST_SCRIPT", script)
        .env("CLISAGE_TEST_SHELL", shell)
        .env("CLISAGE_TEST_PROMPT", original)
        .env("CLISAGE_TEST_OUTPUT", suggestion)
        .env("CLISAGE_TEST_STATUS", status)
        .env("CLISAGE_TEST_MARKER", &marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(b"unrelated terminal input\n");
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!marker.exists(), "the suggestion must never be evaluated");
    let text = String::from_utf8(out.stdout).unwrap();
    let (buffer, cursor) = text.rsplit_once('\n').unwrap();
    (buffer.to_string(), cursor.to_string())
}

#[cfg(unix)]
#[test]
fn widgets_replace_line_without_evaluating_prompt_or_suggestion() {
    let original = "--find files 'quoted' $(exit 92)";
    let suggestion = "printf café > \"$CLISAGE_TEST_MARKER\"";
    for shell in ["zsh", "bash"] {
        let (buffer, cursor) = widget(shell, original, suggestion, "0");
        assert_eq!(buffer, suggestion);
        let expected = if shell == "zsh" {
            suggestion.chars().count()
        } else {
            suggestion.len()
        };
        assert_eq!(cursor, expected.to_string());
    }
}

#[cfg(unix)]
#[test]
fn widgets_preserve_original_line_and_cursor_on_failure_or_empty_output() {
    for shell in ["zsh", "bash"] {
        for (suggestion, status) in [
            ("partial output", "1"),
            ("", "0"),
            ("cancelled output", "130"),
        ] {
            assert_eq!(
                widget(shell, "find large files", suggestion, status),
                ("find large files".into(), "2".into())
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn empty_input_does_not_request_a_suggestion() {
    for shell in ["zsh", "bash"] {
        assert_eq!(
            widget(shell, "", "must not appear", "0"),
            ("".into(), "2".into())
        );
    }
}

#[test]
fn zsh_binds_alt_g_in_emacs_and_vi_insert_without_changing_mode() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("integration");
    std::fs::write(&script, integration("zsh")).unwrap();
    let out = Command::new("zsh")
        .args([
            "-dfc",
            r#"
        bindkey -v
        source "$CLISAGE_TEST_SCRIPT"
        bindkey -lL main
        bindkey -M emacs '^[g'
        bindkey -M viins '^[g'
        bindkey -M emacs '^[G'
        bindkey -M viins '^[G'
    "#,
        ])
        .env("CLISAGE_TEST_SCRIPT", script)
        .output()
        .unwrap();
    assert!(out.status.success());
    let output = String::from_utf8(out.stdout).unwrap();
    assert!(output.contains("viins main"));
    assert_eq!(output.matches("_clisage_zsh_widget").count(), 4);
}

#[test]
fn zsh_keeps_failure_messages_visible_after_refreshing_the_prompt() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("integration");
    std::fs::write(&script, integration("zsh")).unwrap();
    let fake = dir.path().join("clisage");
    std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let zsh = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("zsh"))
        .find(|path| path.is_file())
        .expect("install Zsh for integration tests");
    let output = Command::new(zsh)
        .args([
            "-dfc",
            r#"
        zle() {
            case "$1" in
                reset-prompt) visible_message='' ;;
                -M) visible_message=$2 ;;
            esac
        }
        source "$CLISAGE_TEST_SCRIPT"
        BUFFER='fake request'
        CURSOR=2
        visible_message=''
        _clisage_zsh_widget
        print -r -- "$visible_message"
    "#,
        ])
        .env("CLISAGE_TEST_SCRIPT", script)
        .env("PATH", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("request failed or cancelled"));
}

#[test]
fn bash_without_a_terminal_does_not_register_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("integration");
    std::fs::write(&script, integration("bash")).unwrap();
    let out = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-ic",
            r#"
            bind() { printf 'unexpected binding registration'; }
            source "$CLISAGE_TEST_SCRIPT"
        "#,
        ])
        .env("CLISAGE_TEST_SCRIPT", script)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("unexpected binding registration"));
}

#[cfg(unix)]
#[test]
fn actual_alt_g_keeps_suggestions_editable_in_emacs_and_vi_modes() {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::Read,
        os::unix::fs::PermissionsExt,
        sync::mpsc,
        time::{Duration, Instant},
    };

    struct KillOnDrop {
        child: Box<dyn portable_pty::Child + Send + Sync>,
        writer: Option<Box<dyn Write + Send>>,
    }
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            // portable-pty's writer sends a newline/EOF on drop. Terminate
            // the shell first so teardown cannot submit the pending command
            // or block while the shell is still editing its input buffer.
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.writer.take();
        }
    }
    fn read_until(rx: &mpsc::Receiver<Vec<u8>>, pending: &mut Vec<u8>, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(index) = pending
                .windows(needle.len())
                .position(|bytes| bytes == needle.as_bytes())
            {
                pending.drain(..index + needle.len());
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let chunk = rx.recv_timeout(remaining).unwrap_or_else(|_| {
                panic!(
                    "terminal did not display {needle:?}: {}",
                    String::from_utf8_lossy(pending)
                );
            });
            pending.extend_from_slice(&chunk);
        }
    }
    for shell in ["zsh", "bash"] {
        if shell == "bash" {
            let version = Command::new(shell)
                .args(["-c", "printf '%s' \"${BASH_VERSINFO[0]}\""])
                .output()
                .unwrap();
            if String::from_utf8(version.stdout)
                .unwrap()
                .parse::<u32>()
                .unwrap()
                < 4
            {
                continue;
            }
        }
        for mode in ["emacs", "vi"] {
            let dir = tempfile::tempdir().unwrap();
            let script = dir.path().join("integration");
            std::fs::write(&script, integration(shell)).unwrap();
            let fake = dir.path().join("clisage");
            std::fs::write(
                &fake,
                "#!/bin/sh\nprintf '%s\\n' 'printf executed > \"$CLISAGE_TEST_MARKER\"'\n",
            )
            .unwrap();
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
            let marker = dir.path().join("must-not-execute");
            let shell_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
                .map(|path| path.join(shell))
                .find(|path| path.is_file())
                .unwrap();
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: 24,
                    cols: 140,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let mut cmd = CommandBuilder::new(shell_path);
            if shell == "zsh" {
                cmd.args(["-df"]);
            } else {
                cmd.args(["--noprofile", "--norc", "-i"]);
            }
            cmd.env("HOME", dir.path());
            cmd.env("ZDOTDIR", dir.path());
            cmd.env("HISTFILE", "/dev/null");
            cmd.env("INPUTRC", "/dev/null");
            cmd.env("TERM", "xterm");
            cmd.env("PS1", "CLISAGE-TEST> ");
            cmd.env("PATH", dir.path());
            cmd.env("CLISAGE_TEST_SCRIPT", script);
            cmd.env("CLISAGE_TEST_MARKER", &marker);
            let mut child = KillOnDrop {
                child: pair.slave.spawn_command(cmd).unwrap(),
                writer: None,
            };
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            child.writer = Some(pair.master.take_writer().unwrap());
            let writer = child.writer.as_mut().unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buffer = [0; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            if tx.send(buffer[..count].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            let mut pending = Vec::new();
            read_until(&rx, &mut pending, "CLISAGE-TEST> ");
            let editing_mode = match (shell, mode) {
                ("zsh", "emacs") => "bindkey -e",
                ("zsh", _) => "bindkey -v",
                (_, "emacs") => "set -o emacs",
                _ => "set -o vi",
            };
            writeln!(writer, "{editing_mode}; source \"$CLISAGE_TEST_SCRIPT\"").unwrap();
            read_until(&rx, &mut pending, "CLISAGE-TEST> ");
            writer.write_all(b"find large files\x1bg").unwrap();
            read_until(
                &rx,
                &mut pending,
                "printf executed > \"$CLISAGE_TEST_MARKER\"",
            );
            assert!(
                !marker.exists(),
                "{shell} {mode} executed a suggestion without Enter"
            );
            // Discard the pending suggestion without accepting the line.
            writer.write_all(b"\x15").unwrap();
            drop(child);
            assert!(
                !marker.exists(),
                "PTY teardown must not submit a suggestion"
            );
        }
    }
}

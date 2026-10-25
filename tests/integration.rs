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
fn bash_binds_alt_g_in_emacs_and_vi_insert_without_changing_mode() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("integration");
    std::fs::write(&script, integration("bash")).unwrap();
    let out = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-ic",
            r#"
        # Stock macOS Bash 3 lacks READLINE_LINE; the integration explains this.
        if (( BASH_VERSINFO[0] < 4 )); then
            source "$CLISAGE_TEST_SCRIPT"
            exit
        fi
        set -o vi
        source "$CLISAGE_TEST_SCRIPT"
        [[ -o vi ]] || exit 1
        bind -m emacs-standard -X
        bind -m vi-insert -X
    "#,
        ])
        .env("INPUTRC", "/dev/null")
        .env("CLISAGE_TEST_SCRIPT", script)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    if String::from_utf8_lossy(&out.stderr).contains("requires Bash 4+") {
        return;
    }
    assert_eq!(
        String::from_utf8_lossy(&out.stdout)
            .matches("_clisage_bash_widget")
            .count(),
        4
    );
}

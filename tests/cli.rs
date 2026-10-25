use std::{
    io::Write,
    process::{Command, Stdio},
};

fn cli() -> (Command, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_clisage"));
    cmd.env_remove("CLISAGE_API_KEY")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("PATH", "")
        .stdin(Stdio::null());
    (cmd, dir)
}

#[test]
fn help_and_version_work_without_credentials() {
    let (mut cmd, _dir) = cli();
    assert!(cmd.arg("--help").output().unwrap().status.success());
    let (mut cmd, _dir) = cli();
    assert!(cmd.arg("--version").output().unwrap().status.success());
}

#[test]
fn missing_noninteractive_credential_fails_with_guidance() {
    let (mut cmd, _dir) = cli();
    let out = cmd.arg("hello").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("clisage auth login"));
    assert!(out.stdout.is_empty());
}

#[test]
fn config_show_is_secret_free_and_does_not_create_files() {
    let (mut cmd, dir) = cli();
    let out = cmd
        .env("CLISAGE_API_KEY", "sentinel-secret")
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("gpt-4.1-mini"));
    assert!(!text.contains("sentinel-secret"));
    assert!(!dir.path().join("clisage").exists());
}

#[test]
fn auth_status_redacts_environment_secret() {
    let (mut cmd, _dir) = cli();
    let out = cmd
        .env("CLISAGE_API_KEY", "sentinel-secret")
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("environment"));
    assert!(!text.contains("sentinel-secret"));
}

#[test]
fn login_never_consumes_piped_input_as_a_key() {
    let (mut cmd, dir) = cli();
    let mut child = cmd
        .args(["auth", "login"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let written = child.stdin.take().unwrap().write_all(b"sentinel-secret\n");
    // Login may reject the pipe before the parent finishes writing.
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("sentinel-secret"));
    assert!(!dir.path().join("clisage").exists());
}

#[test]
fn config_rejects_plaintext_secrets_and_endpoint_overrides() {
    for bad in [
        "api_key = 'sentinel-secret'",
        "base_url = 'https://evil.invalid'",
    ] {
        let (mut cmd, dir) = cli();
        std::fs::create_dir(dir.path().join("clisage")).unwrap();
        std::fs::write(dir.path().join("clisage/config.toml"), bad).unwrap();
        let out = cmd.args(["config", "show"]).output().unwrap();
        assert!(!out.status.success());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("sentinel-secret"));
    }
}

#[test]
fn invalid_environment_key_is_rejected_without_echoing_it() {
    let (mut cmd, _dir) = cli();
    let out = cmd
        .env("CLISAGE_API_KEY", "fake-secret\ninvalid")
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("fake-secret"));
}

#[test]
fn oversized_stdin_is_rejected_before_authentication() {
    let (mut cmd, _dir) = cli();
    let mut child = cmd
        .arg("summarize")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&vec![b'a'; 1_048_577])
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("exceeds 1 MiB"));
}

#[cfg(unix)]
#[test]
fn fake_store_status_and_logout_use_only_the_scoped_entry() {
    use std::os::unix::fs::PermissionsExt;
    let (mut cmd, dir) = cli();
    let fake = dir.path().join("pass");
    std::fs::write(&fake, "#!/bin/sh\ncase \"$1:$2\" in\nshow:clisage/openai/api.openai.com/default) printf 'fake-store-key\\n';;\nrm:--force) test \"$3\" = clisage/openai/api.openai.com/default;;\n*) exit 1;;\nesac\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = cmd
        .env("PATH", dir.path())
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-store-key"));
    let (mut cmd, _other) = cli();
    assert!(
        cmd.env("PATH", dir.path())
            .args(["auth", "logout"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn failing_store_output_is_never_echoed() {
    use std::os::unix::fs::PermissionsExt;
    let (mut cmd, dir) = cli();
    let fake = dir.path().join("pass");
    std::fs::write(
        &fake,
        "#!/bin/sh\nprintf 'fake-sensitive-store-output' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = cmd
        .env("PATH", dir.path())
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("fake-sensitive-store-output"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("locked"));
}

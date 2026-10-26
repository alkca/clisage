use std::{
    io::Write,
    process::{Command, Stdio},
};

fn cli() -> (Command, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_clisage"));
    cmd.env_remove("CLISAGE_API_KEY")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("HOME", dir.path())
        .env("PASSWORD_STORE_DIR", dir.path().join("store"))
        .env_remove("PASSWORD_STORE_KEY")
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
fn editor_rejects_piped_input_before_authentication() {
    for args in [
        vec!["--editor", "hello"],
        vec!["shell", "--editor"],
        vec!["explain", "--editor"],
    ] {
        let (mut cmd, _dir) = cli();
        let out = cmd.args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("--editor requires a terminal"));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn presentation_flags_are_available_and_mutually_exclusive() {
    let (mut cmd, _dir) = cli();
    let out = cmd.args(["--help"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--markdown"));
    assert!(text.contains("--no-markdown"));
    assert!(text.contains("--editor"));
    let (mut cmd, _dir) = cli();
    let out = cmd
        .args(["--markdown", "--no-markdown", "hello"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
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
    assert!(text.contains("gpt-6-luna"));
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

#[test]
fn command_only_accepts_option_like_prompts_and_emits_nothing_on_failure() {
    let (mut cmd, _dir) = cli();
    let out = cmd
        .args(["shell", "--command-only", "--", "--find large files"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("clisage auth login"));
}

#[cfg(unix)]
#[test]
fn headless_store_access_disables_pinentry_and_preserves_custom_gpg_options() {
    use std::os::unix::fs::PermissionsExt;
    let (mut cmd, dir) = cli();
    let fake = dir.path().join("pass");
    std::fs::write(&fake, "#!/bin/sh\n[ \"$PASSWORD_STORE_GPG_OPTS\" = '--no-auto-key-retrieve --pinentry-mode error' ] || exit 1\nprintf 'fake-store-key\\n'\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = cmd
        .env("PATH", dir.path())
        .env("PASSWORD_STORE_GPG_OPTS", "--no-auto-key-retrieve")
        .args(["auth", "status"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-store-key"));
}

#[test]
fn piped_input_can_supply_the_entire_shell_or_explain_request() {
    for action in ["shell", "explain"] {
        let (mut cmd, _dir) = cli();
        let mut child = cmd
            .arg(action)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"ls -l\n").unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("clisage auth login"));
    }
}

#[cfg(unix)]
#[test]
fn auth_commands_work_even_when_configuration_is_invalid() {
    use std::os::unix::fs::PermissionsExt;
    for action in ["status", "logout"] {
        let (mut cmd, dir) = cli();
        std::fs::create_dir(dir.path().join("clisage")).unwrap();
        std::fs::write(dir.path().join("clisage/config.toml"), "broken config").unwrap();
        let pass = dir.path().join("pass");
        std::fs::write(&pass, "#!/bin/sh\ncase \"$1\" in show) printf 'fake-test-key\\n';; rm) exit 0;; *) exit 1;; esac\n").unwrap();
        std::fs::set_permissions(pass, std::fs::Permissions::from_mode(0o700)).unwrap();
        let out = cmd
            .env("PATH", dir.path())
            .args(["auth", action])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("fake-test-key"));
    }
}

#[cfg(unix)]
#[test]
fn utility_commands_exit_successfully_when_output_pipe_is_closed() {
    use std::os::fd::{FromRawFd, OwnedFd};
    for args in [
        vec!["config", "show"],
        vec!["integration", "bash"],
        vec!["auth", "status"],
        vec!["completions", "bash"],
        vec!["doctor"],
    ] {
        let (mut cmd, _dir) = cli();
        let mut descriptors = [0; 2];
        assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
        let reader = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        let writer = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        drop(reader);
        let out = cmd
            .env("CLISAGE_API_KEY", "fake-test-key")
            .args(args)
            .stdout(Stdio::from(writer))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn invalid_config_reports_location_and_recovery_without_echoing_values() {
    let (mut cmd, dir) = cli();
    std::fs::create_dir(dir.path().join("clisage")).unwrap();
    std::fs::write(
        dir.path().join("clisage/config.toml"),
        "model = 'fake-sensitive-value'\napi_key = 'fake-secret'\n",
    )
    .unwrap();
    let out = cmd.args(["config", "show"]).output().unwrap();
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("line"));
    assert!(error.contains("config.toml"));
    assert!(error.contains("doctor"));
    assert!(!error.contains("fake-sensitive-value"));
    assert!(!error.contains("fake-secret"));
}

#[test]
fn doctor_checks_setup_without_reading_credentials_or_calling_the_provider() {
    let (mut cmd, _dir) = cli();
    let out = cmd
        .arg("doctor")
        .env("CLISAGE_API_KEY", "fake-test-key")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Configuration: OK"));
    assert!(text.contains("Credential: environment"));
    assert!(text.contains("pass: unavailable"));
    assert!(!text.contains("fake-test-key"));
    assert!(!out.status.success());
}

#[test]
fn completions_are_available_without_valid_config_or_credentials() {
    for shell in ["bash", "zsh", "fish"] {
        let (mut cmd, dir) = cli();
        std::fs::create_dir(dir.path().join("clisage")).unwrap();
        std::fs::write(dir.path().join("clisage/config.toml"), "broken").unwrap();
        let out = cmd.args(["completions", shell]).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("clisage"));
        assert!(text.contains("doctor"));
        assert!(text.contains("model"));
    }
}

#[test]
fn explicit_auth_backend_bypasses_config_and_rejects_unsupported_platforms() {
    let (mut cmd, dir) = cli();
    std::fs::create_dir(dir.path().join("clisage")).unwrap();
    std::fs::write(dir.path().join("clisage/config.toml"), "broken").unwrap();
    let out = cmd
        .env("CLISAGE_API_KEY", "fake-test-key")
        .args(["auth", "status", "--backend", "pass"])
        .output()
        .unwrap();
    assert!(out.status.success());
    #[cfg(target_os = "linux")]
    let unsupported = "keychain";
    #[cfg(target_os = "macos")]
    let unsupported = "secret-service";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let (mut cmd, _dir) = cli();
        let out = cmd
            .args(["auth", "logout", "--backend", unsupported])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("not supported"));
    }
}

#[test]
fn doctor_reports_bad_config_and_continues_with_other_checks() {
    let (mut cmd, dir) = cli();
    std::fs::create_dir(dir.path().join("clisage")).unwrap();
    std::fs::write(dir.path().join("clisage/config.toml"), "fake-secret-value").unwrap();
    let out = cmd.arg("doctor").output().unwrap();
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Configuration: ERROR"));
    assert!(text.contains("Shell:"));
    assert!(text.contains("gpg:"));
    assert!(!text.contains("fake-secret-value"));
}

#[test]
fn headless_native_credentials_require_environment_auth_without_opening_a_store() {
    #[cfg(target_os = "linux")]
    let backend = "secret-service";
    #[cfg(target_os = "macos")]
    let backend = "keychain";
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    for action in ["status", "login", "logout"] {
        let (mut cmd, _dir) = cli();
        let out = cmd
            .args(["auth", action, "--backend", backend])
            .env("DBUS_SESSION_BUS_ADDRESS", "fake-sensitive-invalid-address")
            .output()
            .unwrap();
        assert!(!out.status.success());
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("terminal"));
        assert!(!text.contains("fake-sensitive-invalid-address"));
    }
}

#[test]
fn empty_xdg_config_home_uses_the_home_configuration() {
    let (mut cmd, dir) = cli();
    let config = dir.path().join(".config/clisage");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "config_version=1\nmodel='fake-home-model'\ncredential_backend='pass'\n",
    )
    .unwrap();
    let out = cmd
        .env("XDG_CONFIG_HOME", "")
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("fake-home-model"));
}

//! Real Secret Service coverage uses a disposable HOME and a private D-Bus bus.
#![cfg(target_os = "linux")]
use clisage::{Backend, credentials::NativeStore};
use secrecy::{ExposeSecret, SecretString};
use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn secret_service_roundtrip_uses_an_isolated_session() {
    if std::env::var_os("CLISAGE_NATIVE_FIXTURE_CHILD").is_some() {
        use keyring_core::api::CredentialStoreApi;
        let fixture = std::env::var_os("CLISAGE_NATIVE_FIXTURE_HOME").unwrap();
        assert_eq!(std::env::var_os("HOME").unwrap(), fixture);
        assert!(
            std::path::Path::new(&fixture)
                .join("fixture-marker")
                .is_file()
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let store = loop {
            match NativeStore::new(Backend::SecretService) {
                Ok(store) => break store,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50))
                }
                Err(_) => panic!("isolated Secret Service did not start"),
            }
        };
        let unrelated = dbus_secret_service_keyring_store::Store::new()
            .unwrap()
            .build("clisage-test-unrelated", "fixture", None)
            .unwrap();
        unrelated.set_password("fake-unrelated-key").unwrap();
        assert!(store.read().is_err());
        store.store(&SecretString::from("fake-native-key")).unwrap();
        assert_eq!(store.read().unwrap().expose_secret(), "fake-native-key");
        store
            .store(&SecretString::from("fake-replacement-key"))
            .unwrap();
        assert_eq!(
            store.read().unwrap().expose_secret(),
            "fake-replacement-key"
        );
        store.remove().unwrap();
        assert!(store.read().is_err());
        assert_eq!(unrelated.get_password().unwrap(), "fake-unrelated-key");
        unrelated.delete_credential().unwrap();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("runtime");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("fixture-marker"), "isolated test").unwrap();
    let result = Command::new("dbus-run-session").args(["--", "bash", "-c", r#"
        set -euo pipefail
        printf '%s' 'fake-disposable-keyring-password' | gnome-keyring-daemon --foreground --unlock --components=secrets >/dev/null 2>&1 &
        daemon=$!
        trap 'kill "$daemon" 2>/dev/null || :; wait "$daemon" 2>/dev/null || :' EXIT
        "$1" --exact secret_service_roundtrip_uses_an_isolated_session
    "#, "fixture"])
        .arg(std::env::current_exe().unwrap())
        .env("HOME", root.path())
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("XDG_RUNTIME_DIR", runtime)
        .env("CLISAGE_NATIVE_FIXTURE_HOME", root.path())
        .env("CLISAGE_NATIVE_FIXTURE_CHILD", "1")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("GNOME_KEYRING_CONTROL")
        .env_remove("GNOME_KEYRING_PID")
        .env_remove("CLISAGE_API_KEY")
        .output().expect("install dbus-run-session and gnome-keyring for native credential tests");
    // Platform error bodies and credential values are deliberately not printed.
    assert!(
        result.status.success(),
        "isolated Secret Service lifecycle failed; install dbus and gnome-keyring and check local IPC permissions"
    );
}

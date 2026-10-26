//! Native credential stores use the same provider/host/account scope as pass.
//! Raw platform errors can contain secrets and must never escape this module.
use crate::{Backend, Result};
use secrecy::{ExposeSecret, SecretString};

pub const NATIVE_SERVICE: &str = "clisage/openai/api.openai.com";
pub const NATIVE_ACCOUNT: &str = "default";

pub fn check_backend(backend: Backend) -> Result<()> {
    match backend {
        Backend::Pass => Ok(()),
        Backend::Keychain if cfg!(target_os = "macos") => Ok(()),
        Backend::SecretService if cfg!(target_os = "linux") => Ok(()),
        _ => Err("credential backend is not supported on this platform; use pass, keychain on macOS, or secret-service on Linux".into()),
    }
}

pub fn validate_key(key: String) -> Result<SecretString> {
    if key.trim().is_empty() || key.chars().any(char::is_control) {
        return Err("credential is empty or invalid; run clisage auth login".into());
    }
    Ok(SecretString::from(key))
}

pub struct NativeStore {
    entry: keyring_core::Entry,
}

impl NativeStore {
    pub fn new(backend: Backend) -> Result<Self> {
        check_backend(backend)?;
        if backend == Backend::Pass {
            return Err("pass is not a native credential backend".into());
        }
        use keyring_core::api::CredentialStoreApi;
        #[cfg(target_os = "linux")]
        let store = dbus_secret_service_keyring_store::Store::new();
        #[cfg(target_os = "macos")]
        let store = apple_native_keyring_store::keychain::Store::new();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let entry = store.and_then(|store| store.build(NATIVE_SERVICE, NATIVE_ACCOUNT, None))
            .map_err(|_| "unable to initialize native credential store; unlock your keyring or Keychain and check the desktop session")?;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let entry: keyring_core::Entry =
            return Err("native credential store is not supported".into());
        Ok(Self { entry })
    }

    pub fn read(&self) -> Result<SecretString> {
        let key = self.entry.get_password().map_err(|_| "native credential missing, locked, or unavailable; unlock your store and run clisage auth login")?;
        validate_key(key)
    }

    pub fn store(&self, key: &SecretString) -> Result<()> {
        self.entry
            .set_password(key.expose_secret())
            .map_err(|_| "could not store native credential; unlock your credential store")?;
        let stored = self.read()?;
        if stored.expose_secret() != key.expose_secret() {
            return Err("credential storage verification failed".into());
        }
        Ok(())
    }

    pub fn remove(&self) -> Result<()> {
        self.entry
            .delete_credential()
            .map_err(|_| "unable to remove native credential; check your credential store".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mock() -> NativeStore {
        NativeStore {
            entry: {
                use keyring_core::api::CredentialStoreApi;
                keyring_core::mock::Store::new()
                    .unwrap()
                    .build(NATIVE_SERVICE, NATIVE_ACCOUNT, None)
                    .unwrap()
            },
        }
    }

    #[test]
    fn native_lifecycle_uses_only_its_entry() {
        let store = mock();
        let unrelated = mock();
        let key = SecretString::from("fake-native-key");
        assert!(store.read().is_err());
        unrelated
            .store(&SecretString::from("fake-unrelated-key"))
            .unwrap();
        store.store(&key).unwrap();
        assert_eq!(store.read().unwrap().expose_secret(), "fake-native-key");
        store.remove().unwrap();
        assert!(store.read().is_err());
        assert_eq!(
            unrelated.read().unwrap().expose_secret(),
            "fake-unrelated-key"
        );
    }

    #[test]
    fn native_failures_never_expose_platform_errors_or_secret_bytes() {
        let store = mock();
        let mock = store
            .entry
            .as_any()
            .downcast_ref::<keyring_core::mock::Cred>()
            .unwrap();
        for operation in 0..3 {
            mock.set_error(keyring_core::Error::BadEncoding(
                b"fake-sensitive-error".to_vec(),
            ));
            let error = match operation {
                0 => store.read().err().unwrap(),
                1 => store
                    .store(&SecretString::from("fake-test-key"))
                    .unwrap_err(),
                _ => store.remove().unwrap_err(),
            };
            assert!(!error.to_string().contains("fake-sensitive-error"));
        }
        store.entry.set_password("fake-key\ninvalid").unwrap();
        assert!(store.read().is_err());
    }
}

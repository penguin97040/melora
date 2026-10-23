//! The OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service).
//! Calls block, so callers run them with `spawn_blocking`. There is no file fallback.

use crate::secret::Secret;
use std::sync::Mutex;

/// The refresh token, alone, under the app's service name.
pub const REFRESH_TOKEN: &str = "spotify-refresh-token";
/// Non-secret session details as JSON: `{authorizedAt, scopes, accountId, clientId}`.
pub const SESSION: &str = "spotify-session";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// No usable store on this system (or access was refused).
    Unavailable,
    /// The store answered with an unexpected error.
    Failed,
}

pub trait CredentialStore: Send + Sync + 'static {
    fn get(&self, key: &str) -> Result<Option<Secret>, StoreError>;
    fn set(&self, key: &str, value: &Secret) -> Result<(), StoreError>;
    /// Deleting a missing entry succeeds.
    fn delete(&self, key: &str) -> Result<(), StoreError>;
}

pub struct KeyringStore {
    service: &'static str,
}

/// The initialised platform store's name. Only success is cached: a store that isn't reachable
/// yet (for example a Secret Service that starts after the app at login) is retried next time.
static DEFAULT_STORE: Mutex<Option<&'static str>> = Mutex::new(None);

impl KeyringStore {
    pub fn new(service: &'static str) -> Self {
        Self { service }
    }

    /// The platform store's name, for development diagnostics (never values).
    pub fn backend() -> Result<&'static str, StoreError> {
        // Held across initialisation so the default store is set at most once. Every caller
        // needs the store anyway, and these calls already run off the async runtime.
        let mut initialised = DEFAULT_STORE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(name) = *initialised {
            return Ok(name);
        }
        let name = init_default_store()?;
        *initialised = Some(name);
        Ok(name)
    }

    fn entry(&self, key: &str) -> Result<keyring_core::Entry, StoreError> {
        Self::backend()?;
        keyring_core::Entry::new(self.service, key).map_err(map_error)
    }
}

#[cfg(windows)]
fn init_default_store() -> Result<&'static str, StoreError> {
    let store = windows_native_keyring_store::Store::new().map_err(map_error)?;
    keyring_core::set_default_store(store);
    Ok("Windows Credential Manager")
}

#[cfg(target_os = "macos")]
fn init_default_store() -> Result<&'static str, StoreError> {
    let store = apple_native_keyring_store::keychain::Store::new().map_err(map_error)?;
    keyring_core::set_default_store(store);
    Ok("macOS Keychain")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn init_default_store() -> Result<&'static str, StoreError> {
    let store = zbus_secret_service_keyring_store::Store::new().map_err(map_error)?;
    keyring_core::set_default_store(store);
    Ok("Secret Service")
}

fn map_error(error: keyring_core::Error) -> StoreError {
    match error {
        keyring_core::Error::NoStorageAccess(_)
        | keyring_core::Error::NoDefaultStore
        | keyring_core::Error::PlatformFailure(_) => StoreError::Unavailable,
        _ => StoreError::Failed,
    }
}

impl CredentialStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<Secret>, StoreError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn set(&self, key: &str, value: &Secret) -> Result<(), StoreError> {
        self.entry(key)?
            .set_password(value.expose())
            .map_err(map_error)
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

#[cfg(test)]
pub mod mock {
    //! An in-memory store with switchable failures.

    use super::*;
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    #[derive(Clone, Default)]
    pub struct MemoryStore {
        values: Arc<Mutex<HashMap<String, String>>>,
        pub unavailable: Arc<AtomicBool>,
        pub fail_writes: Arc<AtomicBool>,
        pub fail_deletes: Arc<AtomicBool>,
    }

    impl MemoryStore {
        pub fn value(&self, key: &str) -> Option<String> {
            self.values.lock().unwrap().get(key).cloned()
        }

        pub fn insert(&self, key: &str, value: &str) {
            self.values
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.to_owned());
        }

        fn check(&self, flag: &AtomicBool) -> Result<(), StoreError> {
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable);
            }
            if flag.load(Ordering::SeqCst) {
                return Err(StoreError::Failed);
            }
            Ok(())
        }
    }

    impl CredentialStore for MemoryStore {
        fn get(&self, key: &str) -> Result<Option<Secret>, StoreError> {
            self.check(&AtomicBool::new(false))?;
            Ok(self.value(key).map(Secret::new))
        }

        fn set(&self, key: &str, value: &Secret) -> Result<(), StoreError> {
            self.check(&self.fail_writes)?;
            self.insert(key, value.expose());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), StoreError> {
            self.check(&self.fail_deletes)?;
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
}

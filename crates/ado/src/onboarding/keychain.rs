//! PAT storage (0001 FR-7): macOS Keychain generic password, service
//! `dev.adoken.pat`, account `{org}`. Never the DB, settings, or logs.
//!
//! Uses Security.framework directly: shelling out to `security
//! add-generic-password -w <secret>` would expose the PAT in the process list.
//! Calls can block on a Keychain prompt, so [`super::Connector`] always runs
//! them on Tokio's blocking pool.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::{Organization, Pat};

/// Keychain service for PATs.
pub const PAT_SERVICE: &str = "dev.adoken.pat";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct KeychainError(pub String);

/// Where the PAT lives. [`Keychain`] in the app; [`MemorySecretStore`] in tests.
pub trait SecretStore: Send + Sync {
    fn save(&self, org: &Organization, pat: &Pat) -> Result<(), KeychainError>;
    fn load(&self, org: &Organization) -> Result<Option<Pat>, KeychainError>;
    /// `Ok(false)` when there was nothing to delete.
    fn delete(&self, org: &Organization) -> Result<bool, KeychainError>;
}

/// The OS keychain under one service name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keychain {
    service: String,
}

impl Default for Keychain {
    fn default() -> Self {
        Self::new()
    }
}

impl Keychain {
    /// Service [`PAT_SERVICE`].
    pub fn new() -> Self {
        Self::with_service(PAT_SERVICE)
    }

    /// A different service name (keychain tests use a test-only one, 0001 §6).
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    pub fn service(&self) -> &str {
        &self.service
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use zeroize::Zeroizing;

    use super::*;

    /// `errSecItemNotFound`
    const ITEM_NOT_FOUND: i32 = -25300;

    impl SecretStore for Keychain {
        fn save(&self, org: &Organization, pat: &Pat) -> Result<(), KeychainError> {
            set_generic_password(&self.service, org.name(), pat.expose_secret().as_bytes())
                .map_err(|err| KeychainError(err.to_string()))
        }

        fn load(&self, org: &Organization) -> Result<Option<Pat>, KeychainError> {
            match get_generic_password(&self.service, org.name()) {
                Ok(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    let secret = std::str::from_utf8(&bytes)
                        .map_err(|_| KeychainError("the stored token is not text".into()))?;
                    Ok(Some(Pat::from_secret(secret.to_owned())))
                }
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(None),
                Err(err) => Err(KeychainError(err.to_string())),
            }
        }

        fn delete(&self, org: &Organization) -> Result<bool, KeychainError> {
            match delete_generic_password(&self.service, org.name()) {
                Ok(()) => Ok(true),
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(false),
                Err(err) => Err(KeychainError(err.to_string())),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    const UNSUPPORTED: &str = "no credential store on this platform yet (Windows lands in v0.3)";

    impl SecretStore for Keychain {
        fn save(&self, _org: &Organization, _pat: &Pat) -> Result<(), KeychainError> {
            Err(KeychainError(UNSUPPORTED.into()))
        }

        fn load(&self, _org: &Organization) -> Result<Option<Pat>, KeychainError> {
            Err(KeychainError(UNSUPPORTED.into()))
        }

        fn delete(&self, _org: &Organization) -> Result<bool, KeychainError> {
            Err(KeychainError(UNSUPPORTED.into()))
        }
    }
}

/// In-process secret store for tests and gallery stories. Never used by the app.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    items: Mutex<HashMap<String, Pat>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn save(&self, org: &Organization, pat: &Pat) -> Result<(), KeychainError> {
        self.items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org.name().to_owned(), pat.clone());
        Ok(())
    }

    fn load(&self, org: &Organization) -> Result<Option<Pat>, KeychainError> {
        Ok(self
            .items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(org.name())
            .cloned())
    }

    fn delete(&self, org: &Organization) -> Result<bool, KeychainError> {
        Ok(self
            .items
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(org.name())
            .is_some())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Real Keychain round trip under a test-only service (0001 §6).
    #[test]
    fn keychain_round_trip_under_test_service() {
        let keychain =
            Keychain::with_service(format!("dev.adoken.pat.test.{}", std::process::id()));
        let org = Organization::parse("adoken-keychain-test").unwrap();
        let pat = Pat::parse(&"k".repeat(84)).unwrap();
        let _ = keychain.delete(&org);
        assert_eq!(keychain.load(&org), Ok(None));
        keychain.save(&org, &pat).unwrap();
        assert_eq!(keychain.load(&org).unwrap(), Some(pat.clone()));
        // Saving again replaces (the Replace token swap).
        let replacement = Pat::parse(&"r".repeat(84)).unwrap();
        keychain.save(&org, &replacement).unwrap();
        assert_eq!(keychain.load(&org).unwrap(), Some(replacement));
        assert_eq!(keychain.delete(&org), Ok(true));
        assert_eq!(keychain.delete(&org), Ok(false));
        assert_eq!(keychain.load(&org), Ok(None));
    }
}

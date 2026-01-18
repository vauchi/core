//! CLI Configuration

use std::path::PathBuf;

use anyhow::Result;
use vauchi_core::SymmetricKey;

#[cfg(feature = "secure-storage")]
use vauchi_core::storage::secure::{PlatformKeyring, SecureStorage};

#[cfg(not(feature = "secure-storage"))]
use vauchi_core::storage::secure::{FileKeyStorage, SecureStorage};

/// CLI configuration.
#[derive(Debug, Clone)]
pub struct CliConfig {
    /// Data directory for storage.
    pub data_dir: PathBuf,
    /// Relay server URL.
    pub relay_url: String,
}

/// Key name used for SecureStorage.
const KEY_NAME: &str = "storage_key";

impl CliConfig {
    /// Returns the storage path for Vauchi data.
    pub fn storage_path(&self) -> PathBuf {
        self.data_dir.join("data.db")
    }

    /// Returns the identity file path.
    pub fn identity_path(&self) -> PathBuf {
        self.data_dir.join("identity.json")
    }

    /// Returns true if the identity file exists.
    pub fn is_initialized(&self) -> bool {
        self.identity_path().exists()
    }

    /// Loads or creates the storage encryption key using SecureStorage.
    ///
    /// When the `secure-storage` feature is enabled, uses the OS keychain.
    /// Otherwise, falls back to encrypted file storage.
    #[allow(unused_variables)]
    pub fn storage_key(&self) -> Result<SymmetricKey> {
        #[cfg(feature = "secure-storage")]
        {
            let storage = PlatformKeyring::new("vauchi-cli");
            match storage.load_key(KEY_NAME) {
                Ok(Some(bytes)) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(SymmetricKey::from_bytes(arr))
                }
                Ok(Some(_)) => {
                    anyhow::bail!("Invalid storage key length in keychain");
                }
                Ok(None) => {
                    let key = SymmetricKey::generate();
                    storage
                        .save_key(KEY_NAME, key.as_bytes())
                        .map_err(|e| anyhow::anyhow!("Failed to save key to keychain: {}", e))?;
                    Ok(key)
                }
                Err(e) => {
                    anyhow::bail!("Keychain error: {}", e);
                }
            }
        }

        #[cfg(not(feature = "secure-storage"))]
        {
            // Fallback key for encrypting the storage key file
            let fallback_key = SymmetricKey::from_bytes([
                0x57, 0x65, 0x62, 0x42, 0x6f, 0x6f, 0x6b, 0x43, // "VauchiC"
                0x6c, 0x69, 0x4b, 0x65, 0x79, 0x46, 0x61, 0x6c, // "liKeyFal"
                0x6c, 0x62, 0x61, 0x63, 0x6b, 0x56, 0x31, 0x00, // "lbackV1\0"
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // "\0\0\0\0\0\0\0\0"
            ]);

            let key_dir = self.data_dir.join("keys");
            let storage = FileKeyStorage::new(key_dir, fallback_key);

            match storage.load_key(KEY_NAME) {
                Ok(Some(bytes)) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(SymmetricKey::from_bytes(arr))
                }
                Ok(Some(_)) => {
                    anyhow::bail!("Invalid storage key length");
                }
                Ok(None) => {
                    let key = SymmetricKey::generate();
                    storage
                        .save_key(KEY_NAME, key.as_bytes())
                        .map_err(|e| anyhow::anyhow!("Failed to save storage key: {}", e))?;
                    Ok(key)
                }
                Err(e) => {
                    anyhow::bail!("Storage error: {}", e);
                }
            }
        }
    }
}

// INLINE_TEST_REQUIRED: Binary crate without lib.rs - tests cannot be external
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_key_creates_key_on_first_call() {
        let temp_dir = tempdir().unwrap();
        let config = CliConfig {
            data_dir: temp_dir.path().to_path_buf(),
            relay_url: "ws://localhost:8080".to_string(),
        };

        // First call should create a key
        let key = config.storage_key().expect("should create key");

        // Key should be 32 bytes
        assert_eq!(key.as_bytes().len(), 32);
    }

    // Note: When secure-storage feature is enabled, these tests use the OS keychain
    // which is shared across all tests and may not support the same persistence
    // semantics in test environments. These tests verify FileKeyStorage persistence.
    #[cfg(not(feature = "secure-storage"))]
    #[test]
    fn test_storage_key_persists_across_calls() {
        let temp_dir = tempdir().unwrap();
        let config = CliConfig {
            data_dir: temp_dir.path().to_path_buf(),
            relay_url: "ws://localhost:8080".to_string(),
        };

        // First call creates key
        let key1 = config.storage_key().expect("should create key");

        // Second call should return the same key
        let key2 = config.storage_key().expect("should load key");

        assert_eq!(key1.as_bytes(), key2.as_bytes());
    }

    #[cfg(not(feature = "secure-storage"))]
    #[test]
    fn test_storage_key_persists_across_config_instances() {
        let temp_dir = tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();

        // First config instance creates key
        let config1 = CliConfig {
            data_dir: data_dir.clone(),
            relay_url: "ws://localhost:8080".to_string(),
        };
        let key1 = config1.storage_key().expect("should create key");

        // Second config instance with same data_dir loads same key
        let config2 = CliConfig {
            data_dir,
            relay_url: "ws://localhost:8080".to_string(),
        };
        let key2 = config2.storage_key().expect("should load key");

        assert_eq!(key1.as_bytes(), key2.as_bytes());
    }
}

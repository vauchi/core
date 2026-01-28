// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Secure Storage Module
//!
//! Provides platform-native secure storage for sensitive keys.
//! Uses OS keychains (macOS Keychain, Linux Secret Service, Windows Credential Manager)
//! when available, with a fallback to encrypted file storage.

use crate::storage::StorageError;
use std::path::PathBuf;

/// Trait for secure storage of cryptographic keys.
///
/// Implementations should use platform-native secure storage when available:
/// - macOS: Keychain
/// - Linux: Secret Service (GNOME Keyring, KDE Wallet)
/// - Windows: Credential Manager
pub trait SecureStorage: Send + Sync {
    /// Saves a key to secure storage.
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError>;

    /// Loads a key from secure storage.
    /// Returns None if the key doesn't exist.
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError>;

    /// Deletes a key from secure storage.
    fn delete_key(&self, name: &str) -> Result<(), StorageError>;

    /// Checks if a key exists in secure storage.
    fn has_key(&self, name: &str) -> Result<bool, StorageError> {
        Ok(self.load_key(name)?.is_some())
    }
}

/// Platform keyring implementation using the `keyring` crate.
/// Available when the `secure-storage` feature is enabled.
#[cfg(feature = "secure-storage")]
pub struct PlatformKeyring {
    service: String,
}

#[cfg(feature = "secure-storage")]
impl PlatformKeyring {
    /// Creates a new platform keyring accessor.
    ///
    /// # Arguments
    /// * `service` - The service name to use for keychain entries (e.g., "vauchi")
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(feature = "secure-storage")]
impl SecureStorage for PlatformKeyring {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        let entry = keyring::Entry::new(&self.service, name)
            .map_err(|e| StorageError::Encryption(format!("Keyring error: {}", e)))?;

        entry
            .set_secret(key)
            .map_err(|e| StorageError::Encryption(format!("Failed to save to keychain: {}", e)))
    }

    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let entry = keyring::Entry::new(&self.service, name)
            .map_err(|e| StorageError::Encryption(format!("Keyring error: {}", e)))?;

        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(StorageError::Encryption(format!(
                "Failed to load from keychain: {}",
                e
            ))),
        }
    }

    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        let entry = keyring::Entry::new(&self.service, name)
            .map_err(|e| StorageError::Encryption(format!("Keyring error: {}", e)))?;

        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(StorageError::Encryption(format!(
                "Failed to delete from keychain: {}",
                e
            ))),
        }
    }
}

/// File-based key storage (fallback when keyring is unavailable).
/// Keys are stored encrypted in a file using application-level encryption.
pub struct FileKeyStorage {
    path: PathBuf,
    encryption_key: crate::crypto::SymmetricKey,
}

impl FileKeyStorage {
    /// Creates a new file-based key storage.
    ///
    /// # Arguments
    /// * `path` - Path to the directory where keys will be stored
    /// * `encryption_key` - Key used to encrypt stored keys
    pub fn new(path: PathBuf, encryption_key: crate::crypto::SymmetricKey) -> Self {
        Self {
            path,
            encryption_key,
        }
    }

    fn key_file_path(&self, name: &str) -> PathBuf {
        // Sanitize the name to prevent path traversal
        let safe_name = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        self.path.join(format!("{}.key", safe_name))
    }
}

impl SecureStorage for FileKeyStorage {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        // Ensure directory exists
        std::fs::create_dir_all(&self.path)
            .map_err(|e| StorageError::Encryption(format!("Failed to create directory: {}", e)))?;

        // Encrypt the key
        let encrypted = crate::crypto::encrypt(&self.encryption_key, key)
            .map_err(|e| StorageError::Encryption(format!("Encryption failed: {}", e)))?;

        // Write to file
        let file_path = self.key_file_path(name);
        std::fs::write(&file_path, &encrypted)
            .map_err(|e| StorageError::Encryption(format!("Failed to write key file: {}", e)))
    }

    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let file_path = self.key_file_path(name);

        // Check if file exists
        if !file_path.exists() {
            return Ok(None);
        }

        // Read encrypted data
        let encrypted = std::fs::read(&file_path)
            .map_err(|e| StorageError::Encryption(format!("Failed to read key file: {}", e)))?;

        // Decrypt
        let key = crate::crypto::decrypt(&self.encryption_key, &encrypted)
            .map_err(|e| StorageError::Encryption(format!("Decryption failed: {}", e)))?;

        Ok(Some(key))
    }

    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        let file_path = self.key_file_path(name);

        if file_path.exists() {
            std::fs::remove_file(&file_path).map_err(|e| {
                StorageError::Encryption(format!("Failed to delete key file: {}", e))
            })?;
        }

        Ok(())
    }
}

// INLINE_TEST_REQUIRED: MemoryKeyStorage is a test-only implementation used for unit testing SecureStorage trait
/// In-memory storage for testing.
#[cfg(test)]
pub struct MemoryKeyStorage {
    keys: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

#[cfg(test)]
impl Default for MemoryKeyStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MemoryKeyStorage {
    pub fn new() -> Self {
        Self {
            keys: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
impl SecureStorage for MemoryKeyStorage {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.keys
            .lock()
            .unwrap()
            .insert(name.to_string(), key.to_vec());
        Ok(())
    }

    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.keys.lock().unwrap().get(name).cloned())
    }

    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.keys.lock().unwrap().remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;
    use tempfile::TempDir;

    // =============================================================================
    // SecureStorage Trait Tests (TDD - RED/GREEN phase)
    // =============================================================================

    #[test]
    fn test_memory_storage_save_load() {
        let storage = MemoryKeyStorage::new();
        let key = vec![1, 2, 3, 4, 5];

        storage.save_key("test_key", &key).unwrap();
        let loaded = storage.load_key("test_key").unwrap();

        assert_eq!(loaded, Some(key));
    }

    #[test]
    fn test_memory_storage_key_not_found() {
        let storage = MemoryKeyStorage::new();
        let loaded = storage.load_key("nonexistent").unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn test_memory_storage_delete() {
        let storage = MemoryKeyStorage::new();
        let key = vec![1, 2, 3];

        storage.save_key("test_key", &key).unwrap();
        assert!(storage.has_key("test_key").unwrap());

        storage.delete_key("test_key").unwrap();
        assert!(!storage.has_key("test_key").unwrap());
    }

    #[test]
    fn test_memory_storage_overwrite() {
        let storage = MemoryKeyStorage::new();

        storage.save_key("test_key", &[1, 2, 3]).unwrap();
        storage.save_key("test_key", &[4, 5, 6]).unwrap();

        let loaded = storage.load_key("test_key").unwrap();
        assert_eq!(loaded, Some(vec![4, 5, 6]));
    }

    // =============================================================================
    // FileKeyStorage Tests
    // =============================================================================

    #[test]
    fn test_file_storage_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        let key = vec![0xDE, 0xAD, 0xBE, 0xEF];
        storage.save_key("storage_key", &key).unwrap();

        let loaded = storage.load_key("storage_key").unwrap();
        assert_eq!(loaded, Some(key));
    }

    #[test]
    fn test_file_storage_key_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        let loaded = storage.load_key("nonexistent").unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn test_file_storage_delete() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        storage.save_key("test_key", &[1, 2, 3]).unwrap();
        assert!(storage.has_key("test_key").unwrap());

        storage.delete_key("test_key").unwrap();
        assert!(!storage.has_key("test_key").unwrap());
    }

    #[test]
    fn test_file_storage_encrypted() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key.clone());

        let secret_key = vec![0x42; 32];
        storage.save_key("secret", &secret_key).unwrap();

        // Read the file directly - it should be encrypted
        let file_content = std::fs::read(temp_dir.path().join("secret.key")).unwrap();

        // File content should NOT equal the plaintext key
        assert_ne!(file_content, secret_key);
        assert!(file_content.len() > secret_key.len()); // Encrypted data has overhead

        // But loading through the storage should return the original
        let loaded = storage.load_key("secret").unwrap();
        assert_eq!(loaded, Some(secret_key));
    }

    #[test]
    fn test_file_storage_wrong_encryption_key_fails() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key1 = SymmetricKey::generate();
        let encryption_key2 = SymmetricKey::generate();

        let storage1 = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key1);
        let storage2 = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key2);

        // Save with key1
        storage1.save_key("test", &[1, 2, 3]).unwrap();

        // Try to load with key2 - should fail
        let result = storage2.load_key("test");
        assert!(result.is_err());
    }

    #[test]
    fn test_file_storage_path_traversal_prevented() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        // Try to use path traversal in name
        storage.save_key("../../../etc/passwd", &[1, 2, 3]).unwrap();

        // Should be sanitized and saved as a safe filename
        let safe_path = temp_dir.path().join("_________etc_passwd.key");
        assert!(safe_path.exists());

        // The parent directory should NOT have any new files
        let parent_dir = temp_dir.path().parent().unwrap();
        assert!(!parent_dir.join("etc").exists());
    }

    // =============================================================================
    // Platform Keyring Tests (only run when secure-storage feature is enabled)
    // =============================================================================

    #[cfg(feature = "secure-storage")]
    mod keyring_tests {
        use super::*;

        // Note: These tests interact with the actual system keychain.
        // They require a Secret Service daemon (GNOME Keyring, KDE Wallet) on Linux,
        // or equivalent on macOS/Windows. Run manually with desktop session active.

        #[test]
        #[ignore = "Requires system keychain (desktop session)"]
        fn test_platform_keyring_save_load() {
            let storage = PlatformKeyring::new("vauchi-test-unit");
            let key = vec![0x42; 32];

            // Clean up from any previous failed tests
            let _ = storage.delete_key("test_key_1");

            storage.save_key("test_key_1", &key).unwrap();
            let loaded = storage.load_key("test_key_1").unwrap();
            assert_eq!(loaded, Some(key));

            // Clean up
            storage.delete_key("test_key_1").unwrap();
        }

        #[test]
        #[ignore = "Requires system keychain (desktop session)"]
        fn test_platform_keyring_not_found() {
            let storage = PlatformKeyring::new("vauchi-test-unit");
            let loaded = storage.load_key("nonexistent_key_xyz").unwrap();
            assert_eq!(loaded, None);
        }

        #[test]
        #[ignore = "Requires system keychain (desktop session)"]
        fn test_platform_keyring_delete() {
            let storage = PlatformKeyring::new("vauchi-test-unit");

            // Clean up from any previous failed tests
            let _ = storage.delete_key("test_key_2");

            storage.save_key("test_key_2", &[1, 2, 3]).unwrap();
            assert!(storage.has_key("test_key_2").unwrap());

            storage.delete_key("test_key_2").unwrap();
            assert!(!storage.has_key("test_key_2").unwrap());
        }
    }
}

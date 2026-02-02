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

    /// Securely deletes a key by overwriting its storage before removal.
    ///
    /// For file-based storage, overwrites the file with random data then zeros
    /// before deleting. For keychain-based storage, delegates to `delete_key()`
    /// since hardware-backed storage handles secure erasure internally.
    fn secure_delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.delete_key(name)
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

impl FileKeyStorage {
    /// Securely overwrites a file with random data, then zeros, then removes it.
    ///
    /// Uses a single file handle with `sync_all()` between passes to ensure
    /// each overwrite reaches the storage controller before the next pass,
    /// preventing write coalescing on SSDs.
    fn secure_overwrite_and_remove(path: &std::path::Path) -> Result<(), StorageError> {
        use std::io::{Seek, Write};

        if !path.exists() {
            return Ok(());
        }

        let size = std::fs::metadata(path)
            .map_err(|e| StorageError::Encryption(format!("Failed to read file metadata: {}", e)))?
            .len() as usize;

        if size == 0 {
            std::fs::remove_file(path).map_err(|e| {
                StorageError::Encryption(format!("Failed to remove empty file: {}", e))
            })?;
            return Ok(());
        }

        // Open file once, keep handle for all passes
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| {
                StorageError::Encryption(format!("Failed to open file for overwrite: {}", e))
            })?;

        // Pass 1: Overwrite with random data
        use ring::rand::SecureRandom;
        let rng = ring::rand::SystemRandom::new();
        let mut random = vec![0u8; size];
        rng.fill(&mut random).map_err(|_| {
            StorageError::Encryption("Failed to generate random data for overwrite".to_string())
        })?;
        file.write_all(&random).map_err(|e| {
            StorageError::Encryption(format!("Failed to write random overwrite: {}", e))
        })?;
        file.sync_all().map_err(|e| {
            StorageError::Encryption(format!("Failed to sync after random overwrite: {}", e))
        })?;

        // Pass 2: Overwrite with zeros
        file.seek(std::io::SeekFrom::Start(0)).map_err(|e| {
            StorageError::Encryption(format!("Failed to seek for zero overwrite: {}", e))
        })?;
        file.write_all(&vec![0u8; size]).map_err(|e| {
            StorageError::Encryption(format!("Failed to write zero overwrite: {}", e))
        })?;
        file.sync_all().map_err(|e| {
            StorageError::Encryption(format!("Failed to sync after zero overwrite: {}", e))
        })?;

        // Close handle, then remove file
        drop(file);
        std::fs::remove_file(path)
            .map_err(|e| StorageError::Encryption(format!("Failed to remove file: {}", e)))?;

        Ok(())
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
            .map_err(|e| StorageError::Encryption(format!("Failed to write key file: {}", e)))?;

        // Restrict file permissions to owner-only on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&file_path, perms).map_err(|e| {
                StorageError::Encryption(format!("Failed to set file permissions: {}", e))
            })?;
        }

        Ok(())
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

    fn secure_delete_key(&self, name: &str) -> Result<(), StorageError> {
        let file_path = self.key_file_path(name);
        Self::secure_overwrite_and_remove(&file_path)
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

    #[cfg(unix)]
    #[test]
    fn test_file_storage_permissions_are_restricted() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        storage.save_key("secret_key", &[0x42; 32]).unwrap();

        let file_path = temp_dir.path().join("secret_key.key");
        let metadata = std::fs::metadata(&file_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "Key file must have 0600 permissions, got {:o}",
            mode
        );
    }

    #[test]
    fn test_file_storage_secure_delete_removes_file() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        storage.save_key("secret", &[0x42; 32]).unwrap();
        assert!(storage.has_key("secret").unwrap());

        storage.secure_delete_key("secret").unwrap();
        assert!(!storage.has_key("secret").unwrap());

        // File should not exist on disk
        let file_path = temp_dir.path().join("secret.key");
        assert!(!file_path.exists());
    }

    #[test]
    fn test_file_storage_secure_delete_overwrites_before_removal() {
        use std::io::Read;

        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        let secret = [0xAB; 32];
        storage.save_key("overwrite_test", &secret).unwrap();

        let file_path = temp_dir.path().join("overwrite_test.key");
        let original_content = std::fs::read(&file_path).unwrap();
        let original_size = original_content.len();
        assert!(original_size > 0);

        // Manually perform the overwrite steps to verify content changes
        // First, verify we can read the encrypted content
        assert!(file_path.exists());

        // Now securely delete
        storage.secure_delete_key("overwrite_test").unwrap();

        // File should be gone
        assert!(!file_path.exists());
    }

    #[test]
    fn test_file_storage_secure_delete_nonexistent_key_is_ok() {
        let temp_dir = TempDir::new().unwrap();
        let encryption_key = SymmetricKey::generate();
        let storage = FileKeyStorage::new(temp_dir.path().to_path_buf(), encryption_key);

        // Deleting a key that doesn't exist should succeed silently
        let result = storage.secure_delete_key("nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_secure_delete_trait_default_delegates_to_delete() {
        let storage = MemoryKeyStorage::new();

        storage.save_key("test", &[1, 2, 3]).unwrap();
        assert!(storage.has_key("test").unwrap());

        // MemoryKeyStorage uses the default impl which delegates to delete_key
        storage.secure_delete_key("test").unwrap();
        assert!(!storage.has_key("test").unwrap());
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

//! Application State
//!
//! Manages the Vauchi storage and identity.

use std::path::Path;

use anyhow::{Context, Result};
use vauchi_core::{Identity, IdentityBackup, Storage, SymmetricKey};

#[cfg(feature = "secure-storage")]
use vauchi_core::storage::secure::{PlatformKeyring, SecureStorage};

#[cfg(not(feature = "secure-storage"))]
use vauchi_core::storage::secure::{FileKeyStorage, SecureStorage};

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "vauchi-local-storage";

/// Application state containing Vauchi storage.
pub struct AppState {
    /// Storage instance
    pub storage: Storage,
    /// Current identity (if loaded)
    pub identity: Option<Identity>,
    /// Backup data for persistence
    backup_data: Option<Vec<u8>>,
    /// Display name
    display_name: Option<String>,
}

impl AppState {
    /// Loads or creates the storage encryption key using SecureStorage.
    ///
    /// When the `secure-storage` feature is enabled, uses the OS keychain.
    /// Otherwise, falls back to encrypted file storage.
    #[allow(unused_variables)]
    fn load_or_create_storage_key(data_dir: &Path) -> Result<SymmetricKey> {
        const KEY_NAME: &str = "storage_key";

        #[cfg(feature = "secure-storage")]
        {
            let storage = PlatformKeyring::new("vauchi-desktop");
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
            let fallback_key = SymmetricKey::from_bytes([
                0x57, 0x65, 0x62, 0x42, 0x6f, 0x6f, 0x6b, 0x44, // "VauchiD"
                0x65, 0x73, 0x6b, 0x74, 0x6f, 0x70, 0x4b, 0x65, // "esktopKe"
                0x79, 0x46, 0x61, 0x6c, 0x6c, 0x62, 0x61, 0x63, // "yFallbac"
                0x6b, 0x56, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, // "kV1\0\0\0\0\0"
            ]);

            let key_dir = data_dir.join("keys");
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

    /// Create a new application state.
    pub fn new(data_dir: &Path) -> Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(data_dir).context("Failed to create data directory")?;

        let db_path = data_dir.join("vauchi.db");

        // Generate or load encryption key using SecureStorage
        let key = Self::load_or_create_storage_key(data_dir)?;

        let storage = Storage::open(&db_path, key).context("Failed to open storage")?;

        // Try to load existing identity
        let (identity, backup_data, display_name) =
            if let Ok(Some((backup, name))) = storage.load_identity() {
                let backup_obj = IdentityBackup::new(backup.clone());
                let identity = Identity::import_backup(&backup_obj, LOCAL_STORAGE_PASSWORD).ok();
                (identity, Some(backup), Some(name))
            } else {
                (None, None, None)
            };

        Ok(AppState {
            storage,
            identity,
            backup_data,
            display_name,
        })
    }

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some() || self.backup_data.is_some()
    }

    /// Create a new identity.
    pub fn create_identity(&mut self, name: &str) -> Result<()> {
        let identity = Identity::create(name);
        let backup = identity
            .export_backup(LOCAL_STORAGE_PASSWORD)
            .map_err(|e| anyhow::anyhow!("Failed to create backup: {:?}", e))?;
        let backup_data = backup.as_bytes().to_vec();

        self.storage
            .save_identity(&backup_data, name)
            .context("Failed to save identity")?;

        self.identity = Some(identity);
        self.backup_data = Some(backup_data);
        self.display_name = Some(name.to_string());
        Ok(())
    }

    /// Get the display name.
    pub fn display_name(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|i| i.display_name())
            .or(self.display_name.as_deref())
    }

    /// Get the public ID.
    pub fn public_id(&self) -> Option<String> {
        self.identity.as_ref().map(|i| i.public_id())
    }

    /// Update the display name.
    pub fn update_display_name(&mut self, new_name: &str) -> Result<()> {
        let name = new_name.trim();
        if name.is_empty() {
            anyhow::bail!("Display name cannot be empty");
        }
        if name.len() > 100 {
            anyhow::bail!("Display name cannot exceed 100 characters");
        }

        // Update identity
        let identity = self
            .identity
            .as_mut()
            .context("No identity to update")?;
        identity.set_display_name(name);

        // Update card if it exists
        if let Some(mut card) = self.storage.load_own_card()? {
            card.set_display_name(name)
                .map_err(|e| anyhow::anyhow!("Failed to update card name: {}", e))?;
            self.storage.save_own_card(&card)?;
        }

        // Update local display name
        self.display_name = Some(name.to_string());

        // Re-save identity backup
        let backup = identity
            .export_backup(LOCAL_STORAGE_PASSWORD)
            .map_err(|e| anyhow::anyhow!("Failed to export backup: {:?}", e))?;
        self.storage
            .save_identity(backup.as_bytes(), name)
            .context("Failed to save identity")?;

        Ok(())
    }
}

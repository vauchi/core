//! Application State
//!
//! Manages the WebBook storage and identity.

use std::path::Path;

use anyhow::{Context, Result};
use webbook_core::{Identity, IdentityBackup, Storage, SymmetricKey};

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "webbook-local-storage";

/// Application state containing WebBook storage.
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
    /// Create a new application state.
    pub fn new(data_dir: &Path) -> Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(data_dir)
            .context("Failed to create data directory")?;

        let db_path = data_dir.join("webbook.db");

        // Generate or load encryption key
        let key_path = data_dir.join("storage.key");
        let key = if key_path.exists() {
            let key_bytes = std::fs::read(&key_path)
                .context("Failed to read storage key")?;
            if key_bytes.len() != 32 {
                anyhow::bail!("Invalid storage key length");
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key_bytes);
            SymmetricKey::from_bytes(arr)
        } else {
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes())
                .context("Failed to write storage key")?;
            key
        };

        let storage = Storage::open(&db_path, key)
            .context("Failed to open storage")?;

        // Try to load existing identity
        let (identity, backup_data, display_name) = if let Ok(Some((backup, name))) = storage.load_identity() {
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
        let backup = identity.export_backup(LOCAL_STORAGE_PASSWORD)
            .map_err(|e| anyhow::anyhow!("Failed to create backup: {:?}", e))?;
        let backup_data = backup.as_bytes().to_vec();

        self.storage.save_identity(&backup_data, name)
            .context("Failed to save identity")?;

        self.identity = Some(identity);
        self.backup_data = Some(backup_data);
        self.display_name = Some(name.to_string());
        Ok(())
    }

    /// Get the display name.
    pub fn display_name(&self) -> Option<&str> {
        self.identity.as_ref().map(|i| i.display_name())
            .or(self.display_name.as_deref())
    }

    /// Get the public ID.
    pub fn public_id(&self) -> Option<String> {
        self.identity.as_ref().map(|i| i.public_id())
    }
}

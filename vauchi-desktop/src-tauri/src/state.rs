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

/// Default relay URL.
const DEFAULT_RELAY_URL: &str = "wss://relay.vauchi.app";

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
    /// Relay server URL
    relay_url: String,
    /// Data directory for config files
    data_dir: std::path::PathBuf,
    /// Pending device join state (JSON serialized).
    pub pending_device_join: Option<String>,
    /// Pending device link QR data for completing link requests.
    pub pending_device_link_qr: Option<String>,
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

        // Load relay URL with fallback hierarchy:
        // 1. User-configured URL (stored in config file)
        // 2. VAUCHI_RELAY_URL environment variable
        // 3. Default: wss://relay.vauchi.app
        let relay_config_path = data_dir.join("relay_url.txt");
        let relay_url = std::fs::read_to_string(&relay_config_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("VAUCHI_RELAY_URL")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());

        Ok(AppState {
            storage,
            identity,
            backup_data,
            display_name,
            relay_url,
            data_dir: data_dir.to_path_buf(),
            pending_device_join: None,
            pending_device_link_qr: None,
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

    /// Get the relay URL.
    pub fn relay_url(&self) -> &str {
        &self.relay_url
    }

    /// Set the relay URL.
    pub fn set_relay_url(&mut self, url: &str) -> Result<()> {
        let url = url.trim();
        if url.is_empty() {
            anyhow::bail!("Relay URL cannot be empty");
        }
        if !url.starts_with("wss://") && !url.starts_with("ws://") {
            anyhow::bail!("Relay URL must start with wss:// or ws://");
        }

        // Validate URL format
        url::Url::parse(url).context("Invalid URL format")?;

        // Save to config file
        let relay_config_path = self.data_dir.join("relay_url.txt");
        std::fs::write(&relay_config_path, url).context("Failed to save relay URL")?;

        self.relay_url = url.to_string();
        Ok(())
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
        let identity = self.identity.as_mut().context("No identity to update")?;
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

// ===========================================================================
// AppState Tests
// Trace: features/identity_management.feature, contact_card_management.feature
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Mutex to serialize tests that modify VAUCHI_RELAY_URL env var
    static ENV_VAR_MUTEX: Mutex<()> = Mutex::new(());

    /// Create a test app state with isolated data directory.
    fn create_test_state() -> (AppState, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let state = AppState::new(temp_dir.path()).expect("Failed to create state");
        (state, temp_dir)
    }

    // === Identity Management Tests ===
    // Trace: identity_management.feature

    /// Trace: identity_management.feature - New state has no identity
    #[test]
    fn test_new_state_has_no_identity() {
        let (state, _temp) = create_test_state();
        assert!(!state.has_identity());
        assert!(state.display_name().is_none());
        assert!(state.public_id().is_none());
    }

    /// Trace: identity_management.feature - Create new identity
    #[test]
    fn test_create_identity() {
        let (mut state, _temp) = create_test_state();

        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        assert!(state.has_identity());
        assert_eq!(state.display_name(), Some("Alice Smith"));
        assert!(state.public_id().is_some());
    }

    /// Trace: identity_management.feature - Identity persists across state instances
    #[test]
    fn test_identity_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create identity in first state
        {
            let mut state = AppState::new(temp_dir.path()).expect("Failed to create state");
            state
                .create_identity("Alice Smith")
                .expect("Failed to create identity");
        }

        // Load in second state
        {
            let state = AppState::new(temp_dir.path()).expect("Failed to load state");
            assert!(state.has_identity());
            assert_eq!(state.display_name(), Some("Alice Smith"));
        }
    }

    // === Settings Tests ===

    /// Test default relay URL
    #[test]
    fn test_relay_url_default() {
        let _lock = ENV_VAR_MUTEX.lock().unwrap();

        // Save existing env var value
        let saved_env = std::env::var("VAUCHI_RELAY_URL").ok();
        // Remove env var for this test
        std::env::remove_var("VAUCHI_RELAY_URL");

        // Create temp dir and state AFTER removing env var
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let state = AppState::new(temp_dir.path()).expect("Failed to create state");
        let relay_url = state.relay_url().to_string();

        // Restore env var
        if let Some(val) = saved_env {
            std::env::set_var("VAUCHI_RELAY_URL", val);
        }

        assert_eq!(relay_url, "wss://relay.vauchi.app");
    }

    /// Test setting relay URL
    #[test]
    fn test_set_relay_url() {
        let (mut state, _temp) = create_test_state();

        state
            .set_relay_url("wss://custom.relay.example.com")
            .expect("Failed to set relay URL");

        assert_eq!(state.relay_url(), "wss://custom.relay.example.com");
    }

    /// Test relay URL persistence
    #[test]
    fn test_relay_url_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Set relay URL in first state
        {
            let mut state = AppState::new(temp_dir.path()).expect("Failed to create state");
            state
                .set_relay_url("wss://custom.relay.example.com")
                .expect("Failed to set relay URL");
        }

        // Load in second state
        {
            let state = AppState::new(temp_dir.path()).expect("Failed to load state");
            assert_eq!(state.relay_url(), "wss://custom.relay.example.com");
        }
    }

    /// Test invalid relay URL rejected
    #[test]
    fn test_invalid_relay_url_rejected() {
        let (mut state, _temp) = create_test_state();

        let result = state.set_relay_url("invalid-url");
        assert!(result.is_err());
    }

    /// Test empty relay URL rejected
    #[test]
    fn test_empty_relay_url_rejected() {
        let (mut state, _temp) = create_test_state();

        let result = state.set_relay_url("");
        assert!(result.is_err());
    }

    /// Test http URL rejected (must be ws or wss)
    #[test]
    fn test_http_relay_url_rejected() {
        let (mut state, _temp) = create_test_state();

        let result = state.set_relay_url("https://relay.example.com");
        assert!(result.is_err());
    }

    /// Test VAUCHI_RELAY_URL env var is used when no config file exists
    #[test]
    fn test_relay_url_from_env_var() {
        let _lock = ENV_VAR_MUTEX.lock().unwrap();

        // Save existing env var value
        let saved_env = std::env::var("VAUCHI_RELAY_URL").ok();

        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Set env var before creating state
        std::env::set_var("VAUCHI_RELAY_URL", "wss://env.relay.example.com");

        let state = AppState::new(temp_dir.path()).expect("Failed to create state");
        let relay_url = state.relay_url().to_string();

        // Restore or remove env var
        match saved_env {
            Some(val) => std::env::set_var("VAUCHI_RELAY_URL", val),
            None => std::env::remove_var("VAUCHI_RELAY_URL"),
        }

        assert_eq!(relay_url, "wss://env.relay.example.com");
    }

    /// Test config file takes precedence over env var
    #[test]
    fn test_config_file_precedence_over_env_var() {
        let _lock = ENV_VAR_MUTEX.lock().unwrap();

        // Save existing env var value
        let saved_env = std::env::var("VAUCHI_RELAY_URL").ok();

        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Write config file
        let relay_config_path = temp_dir.path().join("relay_url.txt");
        std::fs::write(&relay_config_path, "wss://config.relay.example.com")
            .expect("Failed to write config");

        // Set env var
        std::env::set_var("VAUCHI_RELAY_URL", "wss://env.relay.example.com");

        let state = AppState::new(temp_dir.path()).expect("Failed to create state");
        let relay_url = state.relay_url().to_string();

        // Restore or remove env var
        match saved_env {
            Some(val) => std::env::set_var("VAUCHI_RELAY_URL", val),
            None => std::env::remove_var("VAUCHI_RELAY_URL"),
        }

        // Config file should take precedence
        assert_eq!(relay_url, "wss://config.relay.example.com");
    }

    // === Display Name Tests ===
    // Trace: contact_card_management.feature

    /// Trace: contact_card_management.feature - Update display name
    #[test]
    fn test_update_display_name() {
        let (mut state, _temp) = create_test_state();
        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        state
            .update_display_name("Alice S.")
            .expect("Failed to update name");

        assert_eq!(state.display_name(), Some("Alice S."));
    }

    /// Trace: contact_card_management.feature - Empty display name rejected
    #[test]
    fn test_empty_display_name_rejected() {
        let (mut state, _temp) = create_test_state();
        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let result = state.update_display_name("");
        assert!(result.is_err());
        assert_eq!(state.display_name(), Some("Alice Smith"));
    }

    /// Trace: contact_card_management.feature - Display name too long rejected
    #[test]
    fn test_long_display_name_rejected() {
        let (mut state, _temp) = create_test_state();
        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let long_name = "A".repeat(101);
        let result = state.update_display_name(&long_name);
        assert!(result.is_err());
    }

    /// Trace: contact_card_management.feature - Whitespace-only display name rejected
    #[test]
    fn test_whitespace_display_name_rejected() {
        let (mut state, _temp) = create_test_state();
        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let result = state.update_display_name("   ");
        assert!(result.is_err());
    }

    /// Test display name with leading/trailing whitespace is trimmed
    #[test]
    fn test_display_name_trimmed() {
        let (mut state, _temp) = create_test_state();
        state
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        state
            .update_display_name("  Alice S.  ")
            .expect("Failed to update name");

        assert_eq!(state.display_name(), Some("Alice S."));
    }

    // === Update without identity fails ===

    #[test]
    fn test_update_display_name_without_identity_fails() {
        let (mut state, _temp) = create_test_state();

        let result = state.update_display_name("New Name");
        assert!(result.is_err());
    }
}

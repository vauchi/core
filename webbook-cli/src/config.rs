//! CLI Configuration

use std::path::PathBuf;

use webbook_core::SymmetricKey;

/// CLI configuration.
#[derive(Debug, Clone)]
pub struct CliConfig {
    /// Data directory for storage.
    pub data_dir: PathBuf,
    /// Relay server URL.
    pub relay_url: String,
}

/// Fixed key for local storage (CLI demo only - not for production).
/// This allows data to persist across CLI sessions.
const LOCAL_STORAGE_KEY: [u8; 32] = [
    0x77, 0x65, 0x62, 0x62, 0x6f, 0x6f, 0x6b, 0x2d, // "webbook-"
    0x63, 0x6c, 0x69, 0x2d, 0x73, 0x74, 0x6f, 0x72, // "cli-stor"
    0x61, 0x67, 0x65, 0x2d, 0x6b, 0x65, 0x79, 0x2d, // "age-key-"
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, // "01234567"
];

impl CliConfig {
    /// Returns the storage path for WebBook data.
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

    /// Returns a deterministic storage key for CLI persistence.
    pub fn storage_key(&self) -> SymmetricKey {
        SymmetricKey::from_bytes(LOCAL_STORAGE_KEY)
    }
}

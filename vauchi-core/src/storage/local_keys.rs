// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared local key management utilities.
//!
//! Functions for managing fallback encryption keys and backup passwords
//! on the local filesystem. Used by CLI, TUI, and Desktop frontends.
//!
//! These were extracted from duplicated implementations across frontends
//! to prevent silent divergence (T-M1).

use crate::crypto::SymmetricKey;
use crate::storage::StorageError;
use std::path::Path;

/// Legacy hardcoded backup password used before per-installation passwords.
///
/// Retained for migration: frontends should check for this value and
/// prompt migration to a generated password.
pub const LEGACY_BACKUP_PASSWORD: &str = "vauchi-local-storage";

/// Loads or generates a fallback encryption key for file-based storage.
///
/// When OS keychain is unavailable, this provides a filesystem-backed
/// symmetric key stored at `<data_dir>/.fallback-key`.
///
/// The key file is created with 0o600 permissions on Unix systems.
///
/// # Errors
///
/// Returns `StorageError::InvalidData` if the existing key file has
/// an invalid length (must be exactly 32 bytes).
pub fn load_or_generate_fallback_key(data_dir: &Path) -> Result<SymmetricKey, StorageError> {
    let key_path = data_dir.join(".fallback-key");

    if key_path.exists() {
        // F5 audit fix: wrap in Zeroizing so raw key bytes are cleared on drop
        let bytes = zeroize::Zeroizing::new(
            std::fs::read(&key_path).map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        if bytes.len() != 32 {
            return Err(StorageError::InvalidData(format!(
                "Invalid fallback key length ({}), expected 32. Delete {} to regenerate.",
                bytes.len(),
                key_path.display()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        return Ok(SymmetricKey::from_bytes(arr));
    }

    // Generate a new random key
    let key = SymmetricKey::generate();

    // Ensure parent directory exists
    std::fs::create_dir_all(data_dir)
        .map_err(|e| StorageError::Encryption(format!("Failed to create data directory: {e}")))?;

    std::fs::write(&key_path, key.as_bytes())
        .map_err(|e| StorageError::Encryption(format!("Failed to write fallback key: {e}")))?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| StorageError::Encryption(format!("Failed to set fallback key permissions: {e}")),
        )?;
    }

    Ok(key)
}

/// Loads or generates a hex-encoded backup password.
///
/// Stores a 64-character hex string (32 random bytes) at
/// `<data_dir>/.backup-password`. The file is created with 0o600
/// permissions on Unix systems.
///
/// # Errors
///
/// Returns `StorageError::InvalidData` if the existing password file
/// has an invalid length (must be exactly 64 hex characters).
pub fn load_or_generate_backup_password(data_dir: &Path) -> Result<String, StorageError> {
    let password_path = data_dir.join(".backup-password");

    if password_path.exists() {
        let content = std::fs::read_to_string(&password_path).map_err(|e| {
            StorageError::Encryption(format!("Failed to read backup password: {e}"))
        })?;
        let trimmed = content.trim().to_string();
        if trimmed.len() != 64 {
            return Err(StorageError::InvalidData(format!(
                "Invalid backup password length ({}), expected 64 hex chars. Delete {} to regenerate.",
                trimmed.len(),
                password_path.display()
            )));
        }
        return Ok(trimmed);
    }

    // Generate a new random password (32 random bytes, hex-encoded = 64 chars)
    let key = SymmetricKey::generate();
    let password: String = key
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    std::fs::create_dir_all(data_dir)
        .map_err(|e| StorageError::Encryption(format!("Failed to create data directory: {e}")))?;
    std::fs::write(&password_path, &password)
        .map_err(|e| StorageError::Encryption(format!("Failed to write backup password: {e}")))?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&password_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| StorageError::Encryption(format!("Failed to set backup password permissions: {e}")),
        )?;
    }

    Ok(password)
}

/// Derives a per-data-dir keychain key name using FNV-1a hash.
///
/// Prevents keychain entry collisions when multiple instances use
/// different `--data-dir` values. Format: `storage_key_{hash:016x}`.
///
/// Uses FNV-1a (not cryptographic) — this is a stable hash for
/// namespace scoping, not for security.
pub fn keychain_key_name(data_dir: &Path) -> String {
    let path_str = data_dir.to_string_lossy();
    // FNV-1a hash — stable, well-defined algorithm
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in path_str.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    format!("storage_key_{:016x}", hash)
}

//! Backup Commands
//!
//! Commands for identity backup and restore.

#![allow(dead_code)]

use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Backup result containing encrypted data.
#[derive(Serialize)]
pub struct BackupResult {
    pub success: bool,
    pub data: Option<String>,
    pub error: Option<String>,
}

/// Export the identity as an encrypted backup.
///
/// The backup is encrypted with the provided password using PBKDF2.
/// Requires a strong password (zxcvbn score >= 3).
#[tauri::command]
pub fn export_backup(password: String, state: State<'_, Mutex<AppState>>) -> BackupResult {
    let state = state.lock().unwrap();

    let identity = match state.identity.as_ref() {
        Some(id) => id,
        None => {
            return BackupResult {
                success: false,
                data: None,
                error: Some("No identity to backup".to_string()),
            }
        }
    };

    match identity.export_backup(&password) {
        Ok(backup) => {
            let encoded = STANDARD.encode(backup.as_bytes());
            BackupResult {
                success: true,
                data: Some(encoded),
                error: None,
            }
        }
        Err(e) => BackupResult {
            success: false,
            data: None,
            error: Some(format!("Backup failed: {:?}", e)),
        },
    }
}

/// Import an identity from an encrypted backup.
#[tauri::command]
pub fn import_backup(
    backup_data: String,
    password: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    use webbook_core::IdentityBackup;

    let bytes = STANDARD
        .decode(&backup_data)
        .map_err(|e| format!("Invalid backup data: {}", e))?;

    let backup = IdentityBackup::new(bytes);

    let identity = webbook_core::Identity::import_backup(&backup, &password)
        .map_err(|e| format!("Restore failed: {:?}", e))?;

    let display_name = identity.display_name().to_string();

    // Save to storage
    let state = state.lock().unwrap();
    let backup_data = identity
        .export_backup(&password)
        .map_err(|e| format!("Failed to re-export backup: {:?}", e))?;

    state
        .storage
        .save_identity(backup_data.as_bytes(), &display_name)
        .map_err(|e| format!("Failed to save identity: {:?}", e))?;

    Ok(format!("Restored identity: {}", display_name))
}

/// Check password strength before backup.
#[tauri::command]
pub fn check_password_strength(password: String) -> Result<String, String> {
    use webbook_core::identity::password::{
        password_feedback, validate_password, PasswordStrength,
    };

    match validate_password(&password) {
        Ok(strength) => {
            let level = match strength {
                PasswordStrength::Strong => "strong",
                PasswordStrength::VeryStrong => "very_strong",
                _ => "acceptable",
            };
            Ok(level.to_string())
        }
        Err(_) => {
            let feedback = password_feedback(&password);
            Err(if feedback.is_empty() {
                "Password too weak. Use a longer passphrase.".to_string()
            } else {
                feedback
            })
        }
    }
}

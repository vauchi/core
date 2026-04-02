// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Encrypted contact backup: export and import for all contact kinds.
//!
//! Uses the same crypto primitives as identity backup:
//! Argon2id KDF + XChaCha20-Poly1305 authenticated encryption.
//!
//! ## Format
//!
//! ```text
//! version_byte (0x01)
//! || salt (16 bytes)
//! || ciphertext  (XChaCha20-Poly1305 of JSON payload)
//! ```
//!
//! The JSON payload is a `Vec<ContactBackupEntry>` — one entry per contact.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::contact::{Contact, ImportSource};
use crate::contact_card::ContactCard;
use crate::crypto::{decrypt, derive_key_argon2id, encrypt, random_bytes};

/// Backup format version byte for contact backups.
pub(crate) const CONTACT_BACKUP_VERSION: u8 = 0x01;

/// Error type for contact backup operations.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum BackupError {
    #[error("Backup encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Invalid backup data or wrong password")]
    DecryptionFailed,

    #[error("Unsupported backup version: {0:#x}")]
    UnsupportedVersion(u8),

    #[error("Backup data is too short")]
    TooShort,

    #[error("Serialization failed: {0}")]
    Serialization(String),

    #[error("Deserialization failed: {0}")]
    Deserialization(String),

    #[error("Key derivation failed")]
    KeyDerivation,
}

/// Internal representation of a single contact in the backup JSON payload.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ContactBackupEntry {
    /// Unique ID (public key hex for exchanged, UUID for imported).
    id: String,
    /// Display name.
    display_name: String,
    /// ContactCard serialized as JSON.
    card_json: String,
    /// Kind-specific metadata.
    kind: ContactBackupKind,
}

/// Kind-specific data stored in the backup.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContactBackupKind {
    Exchanged {
        /// Ed25519/X25519 public key (32 bytes, base64-encoded).
        public_key_b64: String,
        /// Shared symmetric key bytes (32 bytes, base64-encoded).
        shared_key_b64: String,
        /// Unix timestamp of the exchange.
        exchange_timestamp: u64,
        /// Whether the fingerprint was manually verified.
        fingerprint_verified: bool,
        /// Visibility rules serialized as JSON.
        visibility_rules_json: String,
        /// Whether this contact is trusted for recovery.
        recovery_trusted: bool,
    },
    Imported {
        /// ImportSource serialized as JSON.
        source_json: String,
        /// Unix timestamp when the contact was imported.
        imported_at: u64,
        /// Original vCard UID for re-import dedup.
        original_uid: Option<String>,
    },
}

impl ContactBackupEntry {
    /// Creates a backup entry from a `Contact`.
    pub(crate) fn from_contact(contact: &Contact) -> Result<Self, BackupError> {
        let card_json = serde_json::to_string(contact.card())
            .map_err(|e| BackupError::Serialization(e.to_string()))?;

        let kind = match contact.kind() {
            crate::contact::kind::ContactKind::Exchanged(ex) => {
                use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                let public_key_b64 = BASE64.encode(ex.public_key);
                let shared_key_b64 = BASE64.encode(ex.shared_key.as_bytes());
                let visibility_rules_json = serde_json::to_string(&ex.visibility_rules)
                    .map_err(|e| BackupError::Serialization(e.to_string()))?;
                ContactBackupKind::Exchanged {
                    public_key_b64,
                    shared_key_b64,
                    exchange_timestamp: ex.exchange_timestamp,
                    fingerprint_verified: ex.fingerprint_verified,
                    visibility_rules_json,
                    recovery_trusted: ex.recovery_trusted,
                }
            }
            crate::contact::kind::ContactKind::Imported(imp) => {
                let source_json = serde_json::to_string(&imp.source)
                    .map_err(|e| BackupError::Serialization(e.to_string()))?;
                ContactBackupKind::Imported {
                    source_json,
                    imported_at: imp.imported_at,
                    original_uid: imp.original_uid.clone(),
                }
            }
        };

        Ok(ContactBackupEntry {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            card_json,
            kind,
        })
    }

    /// Reconstructs a `Contact` from this backup entry.
    pub(crate) fn to_contact(&self) -> Result<Contact, BackupError> {
        let card: ContactCard = serde_json::from_str(&self.card_json)
            .map_err(|e| BackupError::Deserialization(e.to_string()))?;

        match &self.kind {
            ContactBackupKind::Exchanged {
                public_key_b64,
                shared_key_b64,
                exchange_timestamp,
                fingerprint_verified,
                visibility_rules_json,
                recovery_trusted,
            } => {
                use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                let pk_bytes = BASE64
                    .decode(public_key_b64)
                    .map_err(|e| BackupError::Deserialization(e.to_string()))?;
                let public_key: [u8; 32] = pk_bytes
                    .try_into()
                    .map_err(|_| BackupError::Deserialization("bad public key length".into()))?;

                let mut sk_bytes = BASE64
                    .decode(shared_key_b64)
                    .map_err(|e| BackupError::Deserialization(e.to_string()))?;
                let mut sk_arr: [u8; 32] = sk_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| BackupError::Deserialization("bad shared key length".into()))?;
                sk_bytes.zeroize();
                let shared_key = crate::crypto::SymmetricKey::try_from_bytes(sk_arr)
                    .map_err(|_| BackupError::Deserialization("degenerate shared key".into()))?;
                sk_arr.zeroize();

                let visibility_rules = serde_json::from_str(visibility_rules_json)
                    .map_err(|e| BackupError::Deserialization(e.to_string()))?;

                let mut contact = Contact::from_sync_data(
                    public_key,
                    card,
                    shared_key,
                    *exchange_timestamp,
                    *fingerprint_verified,
                    visibility_rules,
                );
                let _ = contact.set_recovery_trusted(*recovery_trusted);
                Ok(contact)
            }
            ContactBackupKind::Imported {
                source_json,
                imported_at,
                original_uid,
            } => {
                let source: ImportSource = serde_json::from_str(source_json)
                    .map_err(|e| BackupError::Deserialization(e.to_string()))?;
                Ok(Contact::from_import_stored(
                    self.id.clone(),
                    card,
                    source,
                    *imported_at,
                    original_uid.clone(),
                ))
            }
        }
    }
}

/// Exports all contacts to an encrypted blob.
///
/// Uses Argon2id KDF + XChaCha20-Poly1305, matching the identity backup format.
///
/// ## Format
///
/// `CONTACT_BACKUP_VERSION (0x01) || salt (16 bytes) || ciphertext`
pub fn export_contact_backup(contacts: &[Contact], password: &str) -> Result<Vec<u8>, BackupError> {
    // Build the entries
    let entries: Vec<ContactBackupEntry> = contacts
        .iter()
        .map(ContactBackupEntry::from_contact)
        .collect::<Result<Vec<_>, _>>()?;

    // Serialize to JSON
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&entries).map_err(|e| BackupError::Serialization(e.to_string()))?,
    );

    // Generate random 16-byte salt
    let salt: [u8; 16] = random_bytes();

    // Derive encryption key
    let key =
        derive_key_argon2id(password.as_bytes(), &salt).map_err(|_| BackupError::KeyDerivation)?;

    // Encrypt
    let ciphertext =
        encrypt(&key, &plaintext).map_err(|e| BackupError::EncryptionFailed(e.to_string()))?;

    // Assemble: version || salt || ciphertext
    let mut out = Vec::with_capacity(1 + 16 + ciphertext.len());
    out.push(CONTACT_BACKUP_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Imports contacts from an encrypted backup blob.
///
/// Returns an error if the data is corrupted, the version is unknown, or the
/// password is wrong (authentication tag mismatch).
pub fn import_contact_backup(data: &[u8], password: &str) -> Result<Vec<Contact>, BackupError> {
    if data.len() < 1 + 16 {
        return Err(BackupError::TooShort);
    }

    let version = data[0];
    match version {
        CONTACT_BACKUP_VERSION => import_v1(&data[1..], password),
        other => Err(BackupError::UnsupportedVersion(other)),
    }
}

/// Import v1: Argon2id + XChaCha20-Poly1305.
fn import_v1(data: &[u8], password: &str) -> Result<Vec<Contact>, BackupError> {
    if data.len() < 16 {
        return Err(BackupError::TooShort);
    }

    // SAFETY: guarded by `data.len() < 16` check above — slice is exactly 16 bytes.
    let salt: [u8; 16] = data[..16]
        .try_into()
        .expect("salt slice is exactly 16 bytes");
    let ciphertext = &data[16..];

    let key =
        derive_key_argon2id(password.as_bytes(), &salt).map_err(|_| BackupError::KeyDerivation)?;

    let plaintext =
        Zeroizing::new(decrypt(&key, ciphertext).map_err(|_| BackupError::DecryptionFailed)?);

    let entries: Vec<ContactBackupEntry> = serde_json::from_slice(&plaintext)
        .map_err(|e| BackupError::Deserialization(e.to_string()))?;

    entries.iter().map(ContactBackupEntry::to_contact).collect()
}

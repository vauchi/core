// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Full backup: v3 format combining identity and contacts in one encrypted envelope.
//!
//! ## Format
//!
//! ```text
//! version_byte (0x03) || salt (16 bytes) || ciphertext (XChaCha20-Poly1305)
//! ```
//!
//! The ciphertext encrypts a JSON `FullBackupEnvelope`. The encryption key is
//! derived via Argon2id followed by HKDF with domain separation `b"vauchi-backup-v3"`,
//! ensuring v3 backup keys are independent of v1/v2 keys even with the same password+salt.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::BackupError;
use super::contact_backup::ContactBackupEntry;
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::kdf::HKDF;
use crate::crypto::{SymmetricKey, decrypt, derive_key_argon2id, encrypt, random_bytes};

/// Backup format version byte for v3 (full backup).
pub(crate) const FULL_BACKUP_VERSION: u8 = 0x03;

/// HKDF domain separation info for v3 backup key derivation.
const HKDF_INFO: &[u8] = b"vauchi-backup-v3";

// ── Input data for export ──────────────────────────────────────────────────

/// Identity data required for a full backup export.
pub struct FullBackupIdentityData {
    pub display_name: String,
    pub master_seed: [u8; 32],
    pub device_index: u32,
    pub device_name: String,
}

impl Drop for FullBackupIdentityData {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

// ── Envelope types (serialized as JSON inside the ciphertext) ──────────────

/// The top-level JSON structure inside the encrypted v3 backup.
#[derive(Debug, Serialize, Deserialize)]
pub struct FullBackupEnvelope {
    pub version: u32,
    pub created_at: u64,
    pub sections: BackupSections,
}

/// All data sections inside a full backup.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupSections {
    pub identity: IdentitySection,
    pub contacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_card: Option<ContactCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelSection>,
}

/// Identity data stored inside the backup envelope.
#[derive(Debug, Serialize, Deserialize)]
pub struct IdentitySection {
    pub display_name: String,
    pub master_seed_b64: String,
    pub device_index: u32,
    pub device_name: String,
}

/// A label (group) stored inside the backup envelope.
#[derive(Debug, Serialize, Deserialize)]
pub struct LabelSection {
    pub label_id: String,
    pub name: String,
    pub contacts: Vec<String>,
}

// ── Key derivation ─────────────────────────────────────────────────────────

/// Derives the v3 encryption key: Argon2id base key -> HKDF domain separation.
fn derive_v3_key(password: &str, salt: &[u8; 16]) -> Result<SymmetricKey, BackupError> {
    let base_key =
        derive_key_argon2id(password.as_bytes(), salt).map_err(|_| BackupError::KeyDerivation)?;

    let prk = HKDF::extract(None, base_key.as_bytes());
    let okm = HKDF::expand(&prk, HKDF_INFO, 32).map_err(|_| BackupError::KeyDerivation)?;

    let mut key_bytes: [u8; 32] = okm
        .as_slice()
        .try_into()
        .map_err(|_| BackupError::KeyDerivation)?;
    let key = SymmetricKey::try_from_bytes(key_bytes).map_err(|_| BackupError::KeyDerivation)?;
    key_bytes.zeroize();
    Ok(key)
}

// ── Export ──────────────────────────────────────────────────────────────────

/// Exports a full v3 backup containing identity, contacts, own card, and labels.
///
/// ## Format
///
/// `FULL_BACKUP_VERSION (0x03) || salt (16 bytes) || ciphertext`
///
/// The ciphertext encrypts a JSON `FullBackupEnvelope`.
pub fn export_full_backup(
    identity_data: &FullBackupIdentityData,
    contacts: &[Contact],
    own_card: Option<&ContactCard>,
    labels: &[(String, String, Vec<String>)],
    password: &str,
    now: u64,
) -> Result<Vec<u8>, BackupError> {
    // Build identity section
    let identity = IdentitySection {
        display_name: identity_data.display_name.clone(),
        master_seed_b64: BASE64.encode(identity_data.master_seed),
        device_index: identity_data.device_index,
        device_name: identity_data.device_name.clone(),
    };

    // Build contact entries via existing ContactBackupEntry serialization
    let contact_values: Vec<serde_json::Value> = contacts
        .iter()
        .map(|c| {
            let entry = ContactBackupEntry::from_contact(c)?;
            serde_json::to_value(&entry).map_err(|e| BackupError::Serialization(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Build label sections
    let label_sections: Vec<LabelSection> = labels
        .iter()
        .map(|(id, name, contact_ids)| LabelSection {
            label_id: id.clone(),
            name: name.clone(),
            contacts: contact_ids.clone(),
        })
        .collect();

    // Assemble envelope
    let envelope = FullBackupEnvelope {
        version: 3,
        created_at: now,
        sections: BackupSections {
            identity,
            contacts: contact_values,
            own_card: own_card.cloned(),
            labels: label_sections,
        },
    };

    // Serialize to JSON
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&envelope).map_err(|e| BackupError::Serialization(e.to_string()))?,
    );

    // Generate random 16-byte salt
    let salt: [u8; 16] = random_bytes();

    // Derive v3 encryption key (Argon2id + HKDF domain separation)
    let key = derive_v3_key(password, &salt)?;

    // Encrypt
    let ciphertext =
        encrypt(&key, &plaintext).map_err(|e| BackupError::EncryptionFailed(e.to_string()))?;

    // Assemble output: version || salt || ciphertext
    let mut out = Vec::with_capacity(1 + 16 + ciphertext.len());
    out.push(FULL_BACKUP_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

// ── Import ─────────────────────────────────────────────────────────────────

/// Imports a full v3 backup, returning the decrypted envelope.
///
/// The caller decides how to restore each section (identity, contacts, etc.).
pub fn import_full_backup(data: &[u8], password: &str) -> Result<FullBackupEnvelope, BackupError> {
    // Minimum: version (1) + salt (16) + nonce (24) + tag (16) + some ciphertext
    if data.len() < 1 + 16 {
        return Err(BackupError::TooShort);
    }

    let version = data[0];
    if version != FULL_BACKUP_VERSION {
        return Err(BackupError::UnsupportedVersion(version));
    }

    let salt: [u8; 16] = data[1..17]
        .try_into()
        .expect("salt slice is exactly 16 bytes");
    let ciphertext = &data[17..];

    // Derive v3 decryption key
    let key = derive_v3_key(password, &salt)?;

    // Decrypt
    let plaintext =
        Zeroizing::new(decrypt(&key, ciphertext).map_err(|_| BackupError::DecryptionFailed)?);

    // Parse JSON envelope
    let envelope: FullBackupEnvelope = serde_json::from_slice(&plaintext)
        .map_err(|e| BackupError::Deserialization(e.to_string()))?;

    Ok(envelope)
}

/// Restores contacts from a v3 backup envelope.
///
/// Deserializes each `serde_json::Value` back through `ContactBackupEntry`.
pub fn restore_contacts_from_envelope(
    envelope: &FullBackupEnvelope,
) -> Result<Vec<Contact>, BackupError> {
    envelope
        .sections
        .contacts
        .iter()
        .map(|v| {
            let entry: ContactBackupEntry = serde_json::from_value(v.clone())
                .map_err(|e| BackupError::Deserialization(e.to_string()))?;
            entry.to_contact()
        })
        .collect()
}

/// Extracts the master seed from a v3 backup envelope's identity section.
///
/// The caller must zeroize the returned array when done.
pub fn extract_master_seed(identity: &IdentitySection) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    let mut decoded = BASE64
        .decode(&identity.master_seed_b64)
        .map_err(|e| BackupError::Deserialization(e.to_string()))?;
    let seed: [u8; 32] = decoded
        .as_slice()
        .try_into()
        .map_err(|_| BackupError::Deserialization("bad master_seed length".into()))?;
    decoded.zeroize();
    Ok(Zeroizing::new(seed))
}

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
//! The ciphertext encrypts a zlib-compressed JSON `FullBackupEnvelope`.
//! Backward compatibility: ciphertexts produced before this change (uncompressed
//! JSON) are still accepted because JSON objects start with `{` (0x7B) and never
//! with the zlib magic byte `0x78`.
//!
//! The encryption key is derived via Argon2id followed by HKDF with domain
//! separation `b"vauchi-backup-v3"`, ensuring v3 backup keys are independent of
//! v1/v2 keys even with the same password+salt.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use super::BackupError;
use super::contact_backup::ContactBackupEntry;
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::kdf::HKDF;
use crate::crypto::{
    SymmetricKey, decrypt, decrypt_with_ad, derive_key_argon2id, encrypt, encrypt_with_ad,
    random_bytes,
};

use super::key_shard::{CEREMONY_ID_LENGTH, GuardianBackupMetadata};

/// Backup format version byte for v3 (full backup).
pub(crate) const FULL_BACKUP_VERSION: u8 = 0x03;

/// Backup format version byte for guardian backups with an explicit random key.
///
/// Format: `0x05 || threshold || count || ceremony_id (16) || salt (16)`
/// `|| key_confirmation (16) || ciphertext`. The complete clear header is
/// AEAD associated data.
pub(crate) const GUARDIAN_BACKUP_VERSION: u8 = 0x05;

const GUARDIAN_BACKUP_PREFIX_LENGTH: usize = 1 + 1 + 1 + CEREMONY_ID_LENGTH + 16;
const KEY_CONFIRMATION_LENGTH: usize = 16;
const GUARDIAN_BACKUP_HEADER_LENGTH: usize =
    GUARDIAN_BACKUP_PREFIX_LENGTH + KEY_CONFIRMATION_LENGTH;

/// Maximum accepted v5 guardian backup size (32 MiB).
pub(crate) const MAX_GUARDIAN_BACKUP_BYTES: usize = 32 * 1024 * 1024;

/// Maximum decompressed JSON size for any full backup (64 MiB).
const MAX_DECOMPRESSED_BACKUP_BYTES: usize = 64 * 1024 * 1024;

/// HKDF domain separation info for v3 backup key derivation.
const HKDF_INFO: &[u8] = b"vauchi-backup-v3";

/// HKDF domain separation info for v5 guardian backup key derivation.
///
/// The key is generated randomly, but we still run it through HKDF with domain
/// separation so that a key accidentally used in another context cannot be
/// directly reused as a v5 backup key.
const GUARDIAN_BACKUP_HKDF_INFO: &[u8] = b"vauchi-backup-v5-guardian";
const GUARDIAN_KEY_CONFIRMATION_DOMAIN: &[u8] = b"vauchi-backup-v5-key-confirmation";

type HmacSha256 = Hmac<Sha256>;

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

/// The top-level JSON structure inside the encrypted v3 backup.
#[derive(Debug, Serialize, Deserialize)]
pub struct FullBackupEnvelope {
    pub version: u32,
    pub created_at: u64,
    pub sections: BackupSections,
}

/// Serde adapter that routes `Option<ContactCard>` through [`BackupContactCard`]
/// so the avatar is serialized as base64 in backups while the public type
/// remains `Option<ContactCard>`.
mod backup_contact_card {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::backup::contact_backup::BackupContactCard;
    use crate::contact_card::ContactCard;

    pub fn serialize<S: Serializer>(
        card: &Option<ContactCard>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match card {
            None => serializer.serialize_none(),
            Some(c) => BackupContactCard::from(c).serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<ContactCard>, D::Error> {
        let backup: Option<BackupContactCard> = Option::deserialize(deserializer)?;
        Ok(backup.map(Into::into))
    }
}

/// All data sections inside a full backup.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupSections {
    pub identity: IdentitySection,
    pub contacts: Vec<serde_json::Value>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "backup_contact_card"
    )]
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

/// Derives the v5 guardian backup encryption key from an explicit random key.
fn derive_guardian_key(key: &SymmetricKey, salt: &[u8; 16]) -> Result<SymmetricKey, BackupError> {
    let prk = HKDF::extract(Some(salt), key.as_bytes());
    let okm = HKDF::expand(&prk, GUARDIAN_BACKUP_HKDF_INFO, 32)
        .map_err(|_| BackupError::KeyDerivation)?;

    let mut key_bytes: [u8; 32] = okm
        .as_slice()
        .try_into()
        .map_err(|_| BackupError::KeyDerivation)?;
    let derived =
        SymmetricKey::try_from_bytes(key_bytes).map_err(|_| BackupError::KeyDerivation)?;
    key_bytes.zeroize();
    Ok(derived)
}

/// zlib magic byte: every zlib header starts with 0x78.
const ZLIB_MAGIC: u8 = 0x78;

/// Compresses plaintext with zlib. Best compression is acceptable because
/// backup creation is not latency-critical.
fn compress_backup(plaintext: &[u8]) -> Result<Vec<u8>, BackupError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    std::io::Write::write_all(&mut encoder, plaintext)
        .map_err(|e| BackupError::Serialization(format!("compression failed: {e}")))?;
    encoder
        .finish()
        .map_err(|e| BackupError::Serialization(format!("compression failed: {e}")))
}

/// Decompresses zlib-compressed plaintext. Falls back to returning the input
/// unchanged if it does not start with the zlib magic byte, preserving
/// compatibility with pre-compression v3 backups.
fn decompress_backup(data: &[u8]) -> Result<Vec<u8>, BackupError> {
    decompress_backup_with_limit(data, MAX_DECOMPRESSED_BACKUP_BYTES)
}

fn decompress_backup_with_limit(
    data: &[u8],
    maximum_output: usize,
) -> Result<Vec<u8>, BackupError> {
    if data.len() > maximum_output && (data.is_empty() || data[0] != ZLIB_MAGIC) {
        return Err(BackupError::TooLarge);
    }
    if data.is_empty() || data[0] != ZLIB_MAGIC {
        return Ok(data.to_vec());
    }
    let decoder = ZlibDecoder::new(data);
    let read_limit = maximum_output.saturating_add(1) as u64;
    let mut limited = std::io::Read::take(decoder, read_limit);
    let initial_capacity = data.len().saturating_mul(4).min(maximum_output);
    let mut out = Vec::with_capacity(initial_capacity);
    std::io::Read::read_to_end(&mut limited, &mut out)
        .map_err(|e| BackupError::Deserialization(format!("decompression failed: {e}")))?;
    if out.len() > maximum_output {
        return Err(BackupError::TooLarge);
    }
    Ok(out)
}

#[cfg(any(feature = "network-rustls", test))]
pub(crate) fn decode_guardian_backup_hex(encoded: &str) -> Result<Vec<u8>, BackupError> {
    decode_guardian_backup_hex_with_limit(encoded, MAX_GUARDIAN_BACKUP_BYTES)
}

#[cfg(any(feature = "network-rustls", test))]
fn decode_guardian_backup_hex_with_limit(
    encoded: &str,
    maximum_output: usize,
) -> Result<Vec<u8>, BackupError> {
    let encoded = encoded.trim();
    if encoded.len() > maximum_output.saturating_mul(2) {
        return Err(BackupError::TooLarge);
    }
    hex::decode(encoded).map_err(|_| BackupError::DecryptionFailed)
}

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
    let identity = IdentitySection {
        display_name: identity_data.display_name.clone(),
        master_seed_b64: BASE64.encode(identity_data.master_seed),
        device_index: identity_data.device_index,
        device_name: identity_data.device_name.clone(),
    };

    let contact_values: Vec<serde_json::Value> = contacts
        .iter()
        .map(|c| {
            let entry = ContactBackupEntry::from_contact(c)?;
            serde_json::to_value(&entry).map_err(|e| BackupError::Serialization(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let label_sections: Vec<LabelSection> = labels
        .iter()
        .map(|(id, name, contact_ids)| LabelSection {
            label_id: id.clone(),
            name: name.clone(),
            contacts: contact_ids.clone(),
        })
        .collect();

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

    let plaintext = Zeroizing::new(
        serde_json::to_vec(&envelope).map_err(|e| BackupError::Serialization(e.to_string()))?,
    );
    if plaintext.len() > MAX_DECOMPRESSED_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    let compressed = compress_backup(&plaintext)?;

    let salt: [u8; 16] = random_bytes();
    let key = derive_v3_key(password, &salt)?;
    let ciphertext =
        encrypt(&key, &compressed).map_err(|e| BackupError::EncryptionFailed(e.to_string()))?;

    let mut out = Vec::with_capacity(1 + 16 + ciphertext.len());
    out.push(FULL_BACKUP_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Exports a full v5 guardian backup using an explicit random symmetric key.
///
/// ## Format
///
/// `version || threshold || count || ceremony_id || salt`
/// `|| key_confirmation || ciphertext`
///
/// The ciphertext encrypts a JSON `FullBackupEnvelope`. The encryption key is
/// provided by the caller (typically a freshly generated key that is then split
/// and distributed as guardian key shards). The 16-byte key confirmation lets
/// recovery reject incorrect reconstructed keys before attempting decompression;
/// the final AEAD open remains authoritative.
pub fn export_guardian_backup(
    identity_data: &FullBackupIdentityData,
    contacts: &[Contact],
    own_card: Option<&ContactCard>,
    labels: &[(String, String, Vec<String>)],
    key: &SymmetricKey,
    metadata: GuardianBackupMetadata,
    now: u64,
) -> Result<Vec<u8>, BackupError> {
    let identity = IdentitySection {
        display_name: identity_data.display_name.clone(),
        master_seed_b64: BASE64.encode(identity_data.master_seed),
        device_index: identity_data.device_index,
        device_name: identity_data.device_name.clone(),
    };

    let contact_values: Vec<serde_json::Value> = contacts
        .iter()
        .map(|c| {
            let entry = ContactBackupEntry::from_contact(c)?;
            serde_json::to_value(&entry).map_err(|e| BackupError::Serialization(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let label_sections: Vec<LabelSection> = labels
        .iter()
        .map(|(id, name, contact_ids)| LabelSection {
            label_id: id.clone(),
            name: name.clone(),
            contacts: contact_ids.clone(),
        })
        .collect();

    let envelope = FullBackupEnvelope {
        version: 5,
        created_at: now,
        sections: BackupSections {
            identity,
            contacts: contact_values,
            own_card: own_card.cloned(),
            labels: label_sections,
        },
    };

    let plaintext = Zeroizing::new(
        serde_json::to_vec(&envelope).map_err(|e| BackupError::Serialization(e.to_string()))?,
    );
    if plaintext.len() > MAX_DECOMPRESSED_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    let compressed = compress_backup(&plaintext)?;

    let salt: [u8; 16] = random_bytes();
    let header = guardian_backup_header(metadata, &salt, key);
    let derived_key = derive_guardian_key(key, &salt)?;
    let ciphertext = encrypt_with_ad(&derived_key, &compressed, &header)
        .map_err(|e| BackupError::EncryptionFailed(e.to_string()))?;

    let mut out = Vec::with_capacity(header.len() + ciphertext.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&ciphertext);
    if out.len() > MAX_GUARDIAN_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(out)
}

fn guardian_backup_header(
    metadata: GuardianBackupMetadata,
    salt: &[u8; 16],
    key: &SymmetricKey,
) -> [u8; GUARDIAN_BACKUP_HEADER_LENGTH] {
    let mut header = [0u8; GUARDIAN_BACKUP_HEADER_LENGTH];
    header[0] = GUARDIAN_BACKUP_VERSION;
    header[1] = metadata.threshold();
    header[2] = metadata.count();
    header[3..3 + CEREMONY_ID_LENGTH].copy_from_slice(metadata.ceremony_id());
    header[3 + CEREMONY_ID_LENGTH..GUARDIAN_BACKUP_PREFIX_LENGTH].copy_from_slice(salt);
    let confirmation = guardian_key_confirmation(key, &header[..GUARDIAN_BACKUP_PREFIX_LENGTH]);
    header[GUARDIAN_BACKUP_PREFIX_LENGTH..].copy_from_slice(&confirmation);
    header
}

fn guardian_key_confirmation(
    key: &SymmetricKey,
    header_prefix: &[u8],
) -> [u8; KEY_CONFIRMATION_LENGTH] {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC accepts 32-byte keys");
    mac.update(GUARDIAN_KEY_CONFIRMATION_DOMAIN);
    mac.update(header_prefix);
    let output = mac.finalize().into_bytes();
    let mut confirmation = [0u8; KEY_CONFIRMATION_LENGTH];
    confirmation.copy_from_slice(&output[..KEY_CONFIRMATION_LENGTH]);
    confirmation
}

fn parse_guardian_backup_header(
    data: &[u8],
) -> Result<(GuardianBackupMetadata, [u8; 16]), BackupError> {
    if data.len() > MAX_GUARDIAN_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    if data.len() < GUARDIAN_BACKUP_HEADER_LENGTH {
        return Err(BackupError::TooShort);
    }
    if data[0] != GUARDIAN_BACKUP_VERSION {
        return Err(BackupError::UnsupportedVersion(data[0]));
    }
    let ceremony_id = data[3..3 + CEREMONY_ID_LENGTH]
        .try_into()
        .map_err(|_| BackupError::TooShort)?;
    let metadata = GuardianBackupMetadata::new(data[1], data[2], ceremony_id)
        .map_err(|_| BackupError::KeyShard("Invalid guardian backup metadata".into()))?;
    let salt = data[3 + CEREMONY_ID_LENGTH..GUARDIAN_BACKUP_PREFIX_LENGTH]
        .try_into()
        .map_err(|_| BackupError::TooShort)?;
    Ok((metadata, salt))
}

/// Parses the public recovery metadata from a v5 guardian backup header.
///
/// The values are authenticated only after [`import_guardian_backup`] opens
/// the ciphertext. Recovery may use them to bound work and select candidate
/// shards, but must not release plaintext until that authentication succeeds.
#[cfg(any(feature = "network-rustls", test))]
pub(crate) fn guardian_backup_metadata(data: &[u8]) -> Result<GuardianBackupMetadata, BackupError> {
    parse_guardian_backup_header(data).map(|(metadata, _)| metadata)
}

/// Checks a reconstructed candidate key against the v5 confirmation field.
///
/// This is a cheap candidate filter only. Callers must still open the backup
/// AEAD before accepting plaintext or treating metadata as authenticated.
#[cfg(feature = "network-rustls")]
pub(crate) fn guardian_backup_key_matches(
    data: &[u8],
    key: &SymmetricKey,
) -> Result<bool, BackupError> {
    parse_guardian_backup_header(data)?;
    let expected = guardian_key_confirmation(key, &data[..GUARDIAN_BACKUP_PREFIX_LENGTH]);
    let actual = &data[GUARDIAN_BACKUP_PREFIX_LENGTH..GUARDIAN_BACKUP_HEADER_LENGTH];
    let difference = expected
        .iter()
        .zip(actual)
        .fold(0u8, |difference, (&left, &right)| {
            difference | (left ^ right)
        });
    Ok(difference == 0)
}

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

    let key = derive_v3_key(password, &salt)?;

    let decrypted =
        Zeroizing::new(decrypt(&key, ciphertext).map_err(|_| BackupError::DecryptionFailed)?);
    let plaintext = decompress_backup(&decrypted)?;

    let envelope: FullBackupEnvelope = serde_json::from_slice(&plaintext)
        .map_err(|e| BackupError::Deserialization(e.to_string()))?;

    Ok(envelope)
}

/// Imports a full v5 guardian backup using an explicit symmetric key.
///
/// This is the companion to [`export_guardian_backup`]. The caller supplies the
/// key reconstructed from guardian key shards.
pub fn import_guardian_backup(
    data: &[u8],
    key: &SymmetricKey,
) -> Result<FullBackupEnvelope, BackupError> {
    let (_metadata, salt) = parse_guardian_backup_header(data)?;
    let header = &data[..GUARDIAN_BACKUP_HEADER_LENGTH];
    let ciphertext = &data[GUARDIAN_BACKUP_HEADER_LENGTH..];
    let derived_key = derive_guardian_key(key, &salt)?;

    let decrypted = Zeroizing::new(
        decrypt_with_ad(&derived_key, ciphertext, header)
            .map_err(|_| BackupError::DecryptionFailed)?,
    );
    let plaintext = decompress_backup(&decrypted)?;

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

// INLINE_TEST_REQUIRED: guardian header bytes and their AEAD binding are a
// private wire-format contract and must be tested beside the offsets.
#[cfg(test)]
mod tests {
    use super::*;

    fn guardian_backup() -> (Vec<u8>, SymmetricKey) {
        let identity = FullBackupIdentityData {
            display_name: "Alice".into(),
            master_seed: [0x42; 32],
            device_index: 0,
            device_name: "Test device".into(),
        };
        let key = SymmetricKey::generate();
        let metadata = GuardianBackupMetadata::new(2, 3, [0x24; CEREMONY_ID_LENGTH]).unwrap();
        let backup = export_guardian_backup(&identity, &[], None, &[], &key, metadata, 1).unwrap();
        (backup, key)
    }

    // @internal
    #[test]
    fn guardian_backup_aead_authenticates_threshold() {
        let (mut backup, key) = guardian_backup();
        backup[1] = 3;

        assert!(matches!(
            import_guardian_backup(&backup, &key),
            Err(BackupError::DecryptionFailed)
        ));
    }

    // @internal
    #[test]
    fn guardian_backup_aead_authenticates_ceremony_id() {
        let (mut backup, key) = guardian_backup();
        backup[3] ^= 0x80;

        assert!(matches!(
            import_guardian_backup(&backup, &key),
            Err(BackupError::DecryptionFailed)
        ));
    }

    // @internal
    #[test]
    fn guardian_backup_key_confirmation_accepts_only_original_key() {
        let (backup, key) = guardian_backup();
        let wrong_key = SymmetricKey::generate();

        assert!(matches!(
            guardian_backup_key_matches(&backup, &key),
            Ok(true)
        ));
        assert!(matches!(
            guardian_backup_key_matches(&backup, &wrong_key),
            Ok(false)
        ));
    }

    // @internal
    #[test]
    fn guardian_backup_rejects_oversized_input_before_authentication() {
        let oversized = vec![0u8; MAX_GUARDIAN_BACKUP_BYTES + 1];

        assert!(matches!(
            guardian_backup_metadata(&oversized),
            Err(BackupError::TooLarge)
        ));
    }

    // @internal
    #[test]
    fn guardian_backup_hex_rejects_oversized_input_before_decode() {
        assert!(matches!(
            decode_guardian_backup_hex_with_limit("000000", 2),
            Err(BackupError::TooLarge)
        ));
    }

    // @internal
    #[test]
    fn decompression_rejects_output_over_limit() {
        let compressed = compress_backup(&[0u8; 1025]).unwrap();

        assert!(matches!(
            decompress_backup_with_limit(&compressed, 1024),
            Err(BackupError::TooLarge)
        ));
    }
}

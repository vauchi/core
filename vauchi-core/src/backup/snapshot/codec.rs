// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Version 7 is two payloads of one size, each under its own password and
//! salt, both bound to a clear header with a random file id (ADR-080 point
//! 7, spec D4). Version 6, one payload under one salt, still restores.

#![cfg_attr(
    not(test),
    deny(
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

use zeroize::Zeroizing;

use crate::crypto::{
    HKDF, SymmetricKey, decrypt_with_ad, derive_key_argon2id, encrypt_with_ad, random_bytes,
};

use super::super::full_backup::{
    MAX_DECOMPRESSED_BACKUP_BYTES, compress_backup, decompress_backup,
};
use super::super::{BackupError, MAX_FULL_BACKUP_BYTES, import_full_backup};
use super::BackupSnapshot;

pub(crate) const SNAPSHOT_VERSION: u8 = 7;
const V6_VERSION: u8 = 6;
const V6_HEADER_LENGTH: usize = 17;
const V6_HKDF_INFO: &[u8] = b"vauchi-backup-v6-state";
const HKDF_INFO: &[u8] = b"vauchi-backup-v7-payload";
const FILE_ID_LENGTH: usize = 16;
const SALT_LENGTH: usize = 16;
const HEADER_LENGTH: usize = 1 + FILE_ID_LENGTH + 2 * SALT_LENGTH;
const LENGTH_PREFIX: usize = 4;
const SMALLEST_BUCKET: usize = 4096;
/// The tag byte, nonce and tag `encrypt_with_ad` adds to each payload.
const AEAD_OVERHEAD: usize = 1 + 24 + 16;
/// The last bucket: what still fits twice in a file at the file-pick
/// limit, which the backup cap must equal (`limits.rs`). Below it buckets
/// double from the smallest (owner, 2026-11-09).
const TOP_BUCKET: usize = (MAX_FULL_BACKUP_BYTES - HEADER_LENGTH) / 2 - AEAD_OVERHEAD;

fn derive_key(password: &str, salt: &[u8; 16], info: &[u8]) -> Result<SymmetricKey, BackupError> {
    let base =
        derive_key_argon2id(password.as_bytes(), salt).map_err(|_| BackupError::KeyDerivation)?;
    let prk = Zeroizing::new(HKDF::extract(None, base.as_bytes()));
    let expanded =
        Zeroizing::new(HKDF::expand(&prk, info, 32).map_err(|_| BackupError::KeyDerivation)?);
    let bytes = Zeroizing::new(
        expanded
            .as_slice()
            .try_into()
            .map_err(|_| BackupError::KeyDerivation)?,
    );
    SymmetricKey::try_from_bytes(*bytes).map_err(|_| BackupError::KeyDerivation)
}

fn compressed_json(snapshot: &BackupSnapshot) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(snapshot)
            .map_err(|_| BackupError::Serialization("Could not encode backup state".into()))?,
    );
    if plaintext.len() > MAX_DECOMPRESSED_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(Zeroizing::new(compress_backup(&plaintext)?))
}

/// The bucket both payloads are padded to: the smallest power of two that
/// holds the larger framed payload, or the top bucket past the last power
/// of two that fits.
fn bucket_for(largest_compressed: usize) -> Result<usize, BackupError> {
    let framed = largest_compressed
        .checked_add(LENGTH_PREFIX)
        .ok_or(BackupError::TooLarge)?;
    let doubled = framed
        .checked_next_power_of_two()
        .ok_or(BackupError::TooLarge)?
        .max(SMALLEST_BUCKET);
    if doubled <= TOP_BUCKET {
        Ok(doubled)
    } else if framed <= TOP_BUCKET {
        Ok(TOP_BUCKET)
    } else {
        Err(BackupError::TooLarge)
    }
}

fn frame(compressed: &[u8], bucket: usize) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    let length = u32::try_from(compressed.len()).map_err(|_| BackupError::TooLarge)?;
    let mut framed = Zeroizing::new(Vec::with_capacity(bucket));
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(compressed);
    framed.resize(bucket, 0);
    Ok(framed)
}

fn unframe(framed: &[u8]) -> Result<&[u8], BackupError> {
    let (length, rest) = framed
        .split_first_chunk::<LENGTH_PREFIX>()
        .ok_or(BackupError::TooShort)?;
    let length = usize::try_from(u32::from_be_bytes(*length)).map_err(|_| BackupError::TooShort)?;
    rest.get(..length)
        .ok_or_else(|| BackupError::Deserialization("Invalid backup state length".into()))
}

/// One payload's contents: its snapshot and the password that opens it.
pub(crate) struct Payload<'a> {
    pub snapshot: &'a BackupSnapshot,
    pub password: &'a str,
}

/// Writes a v7 file with the real payload and, when given, a decoy one.
/// Without a decoy, the second payload is sealed under a random key. The
/// real payload takes a random slot, so which slot a password opens says
/// nothing about the file.
pub(crate) fn encode_with_decoy(
    real: &Payload<'_>,
    decoy: Option<&Payload<'_>>,
) -> Result<Vec<u8>, BackupError> {
    let real_compressed = compressed_json(real.snapshot)?;
    let decoy_compressed = decoy.map(|d| compressed_json(d.snapshot)).transpose()?;
    let largest = decoy_compressed
        .as_ref()
        .map_or(0, |d| d.len())
        .max(real_compressed.len());
    let bucket = bucket_for(largest)?;

    let file_id: [u8; FILE_ID_LENGTH] = random_bytes();
    let first_salt: [u8; SALT_LENGTH] = random_bytes();
    let second_salt: [u8; SALT_LENGTH] = random_bytes();
    let mut header = Vec::with_capacity(HEADER_LENGTH);
    header.push(SNAPSHOT_VERSION);
    header.extend(file_id);
    header.extend(first_salt);
    header.extend(second_salt);

    let real_first = random_bytes::<1>()[0] & 1 == 0;
    let (real_salt, decoy_salt) = if real_first {
        (&first_salt, &second_salt)
    } else {
        (&second_salt, &first_salt)
    };
    let real_key = derive_key(real.password, real_salt, HKDF_INFO)?;
    let real_sealed = seal(&real_key, &frame(&real_compressed, bucket)?, &header)?;
    let decoy_sealed = match (decoy, decoy_compressed) {
        (Some(decoy), Some(compressed)) => {
            let key = derive_key(decoy.password, decoy_salt, HKDF_INFO)?;
            seal(&key, &frame(&compressed, bucket)?, &header)?
        }
        _ => {
            let key = SymmetricKey::try_from_bytes(random_bytes())
                .map_err(|_| BackupError::KeyDerivation)?;
            seal(&key, &Zeroizing::new(vec![0; bucket]), &header)?
        }
    };
    let sealed = if real_first {
        [real_sealed, decoy_sealed]
    } else {
        [decoy_sealed, real_sealed]
    };

    let mut file = header;
    for payload in sealed {
        file.extend(payload);
    }
    if file.len() > MAX_FULL_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    Ok(file)
}

fn seal(key: &SymmetricKey, framed: &[u8], header: &[u8]) -> Result<Vec<u8>, BackupError> {
    encrypt_with_ad(key, framed, header)
        .map_err(|_| BackupError::EncryptionFailed("Could not seal backup state".into()))
}

pub(crate) fn encode(snapshot: &BackupSnapshot, password: &str) -> Result<Vec<u8>, BackupError> {
    encode_with_decoy(&Payload { snapshot, password }, None)
}

pub(crate) fn decode(data: &[u8], password: &str) -> Result<BackupSnapshot, BackupError> {
    if data.len() > MAX_FULL_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    match data.first() {
        Some(&SNAPSHOT_VERSION) => decode_v7(data, password),
        Some(&V6_VERSION) => decode_v6(data, password),
        Some(_) => BackupSnapshot::from_legacy(import_full_backup(data, password)?),
        None => Err(BackupError::TooShort),
    }
}

/// Derives the key for both payloads before trying either, so a restore
/// takes the same time whichever payload opens, or neither.
fn decode_v7(data: &[u8], password: &str) -> Result<BackupSnapshot, BackupError> {
    let (header, payloads) = data
        .split_first_chunk::<HEADER_LENGTH>()
        .ok_or(BackupError::TooShort)?;
    if payloads.is_empty() || payloads.len() % 2 != 0 {
        return Err(BackupError::DecryptionFailed);
    }
    let (first, second) = payloads.split_at(payloads.len() / 2);
    let salt = |at: usize| -> Result<[u8; SALT_LENGTH], BackupError> {
        header
            .get(at..at.saturating_add(SALT_LENGTH))
            .and_then(|s| s.try_into().ok())
            .ok_or(BackupError::TooShort)
    };
    let first_salt = salt(1 + FILE_ID_LENGTH)?;
    let second_salt = salt(1 + FILE_ID_LENGTH + SALT_LENGTH)?;
    let keys = [
        derive_key(password, &first_salt, HKDF_INFO)?,
        derive_key(password, &second_salt, HKDF_INFO)?,
    ];
    let opened = [
        decrypt_with_ad(&keys[0], first, header),
        decrypt_with_ad(&keys[1], second, header),
    ];
    let framed = opened
        .into_iter()
        .find_map(Result::ok)
        .map(Zeroizing::new)
        .ok_or(BackupError::DecryptionFailed)?;
    let compressed = unframe(&framed)?;
    let plaintext = Zeroizing::new(decompress_backup(compressed)?);
    parse_snapshot(&plaintext, SNAPSHOT_VERSION)
}

fn decode_v6(data: &[u8], password: &str) -> Result<BackupSnapshot, BackupError> {
    let (header, ciphertext) = data
        .split_first_chunk::<V6_HEADER_LENGTH>()
        .ok_or(BackupError::TooShort)?;
    let salt = header
        .get(1..)
        .and_then(|s| s.try_into().ok())
        .ok_or(BackupError::TooShort)?;
    let key = derive_key(password, &salt, V6_HKDF_INFO)?;
    let decrypted = Zeroizing::new(
        decrypt_with_ad(&key, ciphertext, header).map_err(|_| BackupError::DecryptionFailed)?,
    );
    let plaintext = Zeroizing::new(decompress_backup(&decrypted)?);
    parse_snapshot(&plaintext, V6_VERSION)
}

fn parse_snapshot(plaintext: &[u8], version: u8) -> Result<BackupSnapshot, BackupError> {
    let snapshot: BackupSnapshot = serde_json::from_slice(plaintext)
        .map_err(|_| BackupError::Deserialization("Invalid backup state".into()))?;
    if snapshot.version != u32::from(version) {
        return Err(BackupError::Deserialization(
            "Invalid backup state version".into(),
        ));
    }
    Ok(snapshot)
}

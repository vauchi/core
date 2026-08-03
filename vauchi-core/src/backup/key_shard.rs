// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Guardian key shards for backup recovery.
//!
//! Implements Shamir's Secret Sharing over the random 32-byte v3 backup
//! encryption key, with each share sealed to a guardian's X25519 public key.
//!
//! # Wire format
//!
//! `BackupKeyShard` serializes as:
//! `version (1) || threshold (1) || count (1) || ceremony_id (16)`
//! `|| index (1) || value (32)`
//!
//! `SealedBackupKeyShard` is the raw output of [`crate::recovery::sealed_box`]
//! (`ephemeral_pk || nonce || ciphertext+tag`), encrypting a serialized
//! `BackupKeyShard`.
//!
//! # Usage
//!
//! 1. Generate a random v3 backup key: [`BackupKey::generate`].
//! 2. Split it: [`split_backup_key`] → `Vec<BackupKeyShard>`.
//! 3. Seal each share to a guardian: [`seal_share_for_guardian`].
//! 4. Later, guardians decrypt their shares: [`open_share_for_guardian`].
//! 5. Reconstruct the key from any threshold of shares: [`reconstruct_backup_key`].

use std::fmt;
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::BackupError;
use crate::crypto::shamir::{self, ShamirError, Share};
use crate::crypto::{SymmetricKey, random_bytes};
use crate::recovery::sealed_box;

/// Version byte for `BackupKeyShard` serialization.
const SHARD_VERSION: u8 = 2;

/// Byte length of the random identifier shared by one backup and its shards.
pub const CEREMONY_ID_LENGTH: usize = 16;

/// Authenticated public parameters for one guardian backup ceremony.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardianBackupMetadata {
    threshold: u8,
    count: u8,
    ceremony_id: [u8; CEREMONY_ID_LENGTH],
}

impl GuardianBackupMetadata {
    /// Generates metadata with a fresh random ceremony identifier.
    pub fn generate(config: KeyShardConfig) -> Self {
        Self {
            threshold: config.threshold(),
            count: config.count(),
            ceremony_id: random_bytes(),
        }
    }

    /// Validates metadata parsed from a wire format.
    pub fn new(
        threshold: u8,
        count: u8,
        ceremony_id: [u8; CEREMONY_ID_LENGTH],
    ) -> Result<Self, KeyShardError> {
        let config = KeyShardConfig::new(threshold, count)?;
        Ok(Self {
            threshold: config.threshold(),
            count: config.count(),
            ceremony_id,
        })
    }

    /// Minimum number of distinct shares needed for recovery.
    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    /// Total number of shares generated for the ceremony.
    pub fn count(&self) -> u8 {
        self.count
    }

    /// Random public identifier that separates independent backup ceremonies.
    pub fn ceremony_id(&self) -> &[u8; CEREMONY_ID_LENGTH] {
        &self.ceremony_id
    }
}

/// A single guardian key shard.
///
/// Represents one `(index, value)` share of the Shamir split. The `value`
/// field is zeroized on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct BackupKeyShard {
    #[zeroize(skip)]
    metadata: GuardianBackupMetadata,
    /// Non-zero byte index (x-coordinate).
    index: u8,
    /// 32-byte share value (y-coordinate).
    value: [u8; 32],
}

impl fmt::Debug for BackupKeyShard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupKeyShard")
            .field("index", &self.index)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

impl BackupKeyShard {
    fn new(
        metadata: GuardianBackupMetadata,
        index: u8,
        value: [u8; 32],
    ) -> Result<Self, KeyShardError> {
        if index == 0 || index > metadata.count() {
            return Err(KeyShardError::InvalidFormat);
        }
        Ok(Self {
            metadata,
            index,
            value,
        })
    }

    /// Returns the authenticated ceremony metadata carried by this shard.
    pub fn metadata(&self) -> GuardianBackupMetadata {
        self.metadata
    }

    /// Returns the non-zero Shamir x-coordinate.
    pub fn index(&self) -> u8 {
        self.index
    }

    /// Serializes the shard to a compact byte representation.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + 1 + 1 + CEREMONY_ID_LENGTH + 1 + 32);
        bytes.push(SHARD_VERSION);
        bytes.push(self.metadata.threshold());
        bytes.push(self.metadata.count());
        bytes.extend_from_slice(self.metadata.ceremony_id());
        bytes.push(self.index);
        bytes.extend_from_slice(&self.value);
        bytes
    }

    /// Deserializes a shard from bytes.
    ///
    /// # Errors
    /// Returns [`KeyShardError::InvalidFormat`] if the bytes are malformed or
    /// the version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyShardError> {
        if bytes.len() != 1 + 1 + 1 + CEREMONY_ID_LENGTH + 1 + 32 {
            return Err(KeyShardError::InvalidFormat);
        }

        let version = bytes[0];
        if version != SHARD_VERSION {
            return Err(KeyShardError::UnsupportedVersion(version));
        }

        let ceremony_id = bytes[3..3 + CEREMONY_ID_LENGTH]
            .try_into()
            .map_err(|_| KeyShardError::InvalidFormat)?;
        let metadata = GuardianBackupMetadata::new(bytes[1], bytes[2], ceremony_id)?;
        let index_offset = 3 + CEREMONY_ID_LENGTH;
        let index = bytes[index_offset];

        let value: [u8; 32] = bytes[index_offset + 1..]
            .try_into()
            .map_err(|_| KeyShardError::InvalidFormat)?;

        Self::new(metadata, index, value)
    }

    /// Converts this shard to the internal Shamir [`Share`] representation.
    fn to_share(&self) -> Result<Share, KeyShardError> {
        Share::new(self.index, self.value).map_err(Into::into)
    }

    /// Creates a `BackupKeyShard` from an internal Shamir [`Share`].
    fn from_share(metadata: GuardianBackupMetadata, share: &Share) -> Result<Self, KeyShardError> {
        Self::new(metadata, share.index(), *share.value())
    }
}

/// Errors specific to key shard operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyShardError {
    #[error("Invalid key shard format")]
    InvalidFormat,

    #[error("Unsupported key shard version: {0}")]
    UnsupportedVersion(u8),

    #[error("Shamir error: {0}")]
    Shamir(#[from] ShamirError),

    #[error("Sealed-box error: {0}")]
    SealedBox(String),

    #[error("Invalid backup key length")]
    InvalidKeyLength,

    #[error("Invalid backup key: {0}")]
    InvalidKey(String),
}

impl From<KeyShardError> for BackupError {
    fn from(err: KeyShardError) -> Self {
        BackupError::KeyShard(err.to_string())
    }
}

/// The random 32-byte backup encryption key used for v3 guardian backups.
///
/// This type wraps a [`SymmetricKey`] and provides conversion to/from raw bytes
/// for Shamir splitting. The key is zeroized on drop.
#[derive(Clone, Debug, Zeroize, ZeroizeOnDrop)]
pub struct BackupKey {
    key: SymmetricKey,
}

impl BackupKey {
    /// Generates a fresh random backup key.
    pub fn generate() -> Self {
        Self {
            key: SymmetricKey::generate(),
        }
    }

    /// Creates a backup key from an existing symmetric key.
    pub fn from_symmetric_key(key: SymmetricKey) -> Self {
        Self { key }
    }

    /// Creates a backup key from raw bytes.
    ///
    /// # Errors
    /// Returns [`KeyShardError::InvalidKeyLength`] if the slice is not 32 bytes,
    /// or [`KeyShardError::InvalidKey`] if the bytes form a degenerate (all-zeros)
    /// key.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyShardError> {
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| KeyShardError::InvalidKeyLength)?;
        let key = SymmetricKey::try_from_bytes(array)
            .map_err(|_| KeyShardError::InvalidKey("degenerate key".into()))?;
        Ok(Self { key })
    }

    /// Returns the underlying symmetric key.
    pub fn symmetric_key(&self) -> &SymmetricKey {
        &self.key
    }

    /// Returns a clone of the underlying symmetric key.
    pub fn to_symmetric_key(&self) -> SymmetricKey {
        self.key.clone()
    }

    /// Returns the raw key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.key.as_bytes()
    }
}

/// Parameters for a guardian backup shard setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyShardConfig {
    /// Minimum shares needed to reconstruct the key.
    threshold: u8,
    /// Total number of shares to generate.
    count: u8,
}

impl KeyShardConfig {
    /// Default guardian backup configuration: 2-of-3.
    pub const DEFAULT: Self = Self {
        threshold: 2,
        count: 3,
    };

    /// Minimum allowed threshold.
    pub const MIN_THRESHOLD: u8 = 2;
    /// Maximum allowed total shares.
    pub const MAX_COUNT: u8 = 10;

    /// Creates a new configuration with validation.
    ///
    /// # Errors
    /// Returns [`KeyShardError::Shamir`] if parameters are outside the allowed
    /// range `2 <= threshold <= count <= 10`.
    pub fn new(threshold: u8, count: u8) -> Result<Self, KeyShardError> {
        // Match the constraints enforced by Shamir's Secret Sharing.
        if threshold < Self::MIN_THRESHOLD {
            return Err(ShamirError::ThresholdTooLow(threshold).into());
        }
        if count < Self::MIN_THRESHOLD {
            return Err(ShamirError::CountTooLow(count).into());
        }
        if count > Self::MAX_COUNT {
            return Err(ShamirError::CountTooHigh(count).into());
        }
        if threshold > count {
            return Err(ShamirError::ThresholdExceedsCount(threshold, count).into());
        }
        Ok(Self { threshold, count })
    }

    /// Minimum shares needed to reconstruct the key.
    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    /// Total number of shares generated.
    pub fn count(&self) -> u8 {
        self.count
    }
}

/// Splits a backup key into guardian shares.
///
/// # Errors
/// Returns [`KeyShardError::Shamir`] if parameters are invalid.
pub fn split_backup_key(
    key: &BackupKey,
    metadata: GuardianBackupMetadata,
) -> Result<Vec<BackupKeyShard>, KeyShardError> {
    let shares = shamir::split(key.as_bytes(), metadata.threshold(), metadata.count())?;
    shares
        .iter()
        .map(|share| BackupKeyShard::from_share(metadata, share))
        .collect()
}

/// Reconstructs a backup key from shares carrying one validated ceremony.
///
/// # Errors
/// Returns an error if the shares are insufficient, malformed, or carry
/// different ceremony metadata.
pub fn reconstruct_backup_key(shards: &[BackupKeyShard]) -> Result<BackupKey, KeyShardError> {
    let metadata = shards
        .first()
        .ok_or(KeyShardError::Shamir(ShamirError::InsufficientShares {
            required: KeyShardConfig::MIN_THRESHOLD,
            got: 0,
        }))?
        .metadata();
    if shards.iter().any(|shard| shard.metadata() != metadata) {
        return Err(KeyShardError::InvalidFormat);
    }
    let shares: Vec<Share> = shards
        .iter()
        .map(BackupKeyShard::to_share)
        .collect::<Result<_, _>>()?;
    let secret = Zeroizing::new(shamir::reconstruct(&shares, metadata.threshold())?);
    BackupKey::from_bytes(secret.as_slice())
}

/// Seals a key shard for a guardian using their X25519 public key.
///
/// Returns a sealed blob that can only be opened by the guardian's secret key.
///
/// # Errors
/// Returns [`KeyShardError::SealedBox`] if the public key is invalid or
/// encryption fails.
pub fn seal_share_for_guardian(
    shard: &BackupKeyShard,
    guardian_pk: &PublicKey,
) -> Result<Vec<u8>, KeyShardError> {
    let plaintext = Zeroizing::new(shard.to_bytes());
    sealed_box::seal(&plaintext, guardian_pk).map_err(|e| KeyShardError::SealedBox(e.to_string()))
}

/// Opens a sealed key shard using the guardian's X25519 secret key.
///
/// # Errors
/// Returns [`KeyShardError::SealedBox`] if decryption fails, or
/// [`KeyShardError::InvalidFormat`] if the decrypted payload is malformed.
pub fn open_share_for_guardian(
    sealed: &[u8],
    guardian_sk: &StaticSecret,
) -> Result<BackupKeyShard, KeyShardError> {
    let plaintext = Zeroizing::new(
        sealed_box::open(sealed, guardian_sk)
            .map_err(|e| KeyShardError::SealedBox(e.to_string()))?,
    );
    BackupKeyShard::from_bytes(&plaintext)
}

// INLINE_TEST_REQUIRED: share serialization, sealed-box encryption, and the full
// guardian backup key flow are tightly coupled to private wire formats; inline
// tests validate these boundaries where the types are defined.
#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use x25519_dalek::StaticSecret;

    fn guardian_keypair() -> (StaticSecret, PublicKey) {
        let sk = StaticSecret::random_from_rng(OsRng);
        let pk = PublicKey::from(&sk);
        (sk, pk)
    }

    fn metadata(threshold: u8, count: u8) -> GuardianBackupMetadata {
        GuardianBackupMetadata::new(threshold, count, [0x42; CEREMONY_ID_LENGTH]).unwrap()
    }

    // @internal
    #[test]
    fn backup_key_shard_serialization_round_trip() {
        let shard = BackupKeyShard::new(metadata(2, 10), 7, [0xABu8; 32]).unwrap();
        let bytes = shard.to_bytes();
        let parsed = BackupKeyShard::from_bytes(&bytes).unwrap();
        assert_eq!(shard, parsed);
    }

    // @internal
    #[test]
    fn backup_key_shard_debug_redacts_value() {
        let shard = BackupKeyShard::new(metadata(2, 3), 1, [0xAB; 32]).unwrap();

        let debug = format!("{shard:?}");

        assert_eq!(debug, "BackupKeyShard { index: 1, value: \"[REDACTED]\" }");
        assert!(!debug.contains("171"));
    }

    // @internal
    #[test]
    fn backup_key_shard_rejects_zero_index() {
        let shard = BackupKeyShard::new(metadata(2, 3), 1, [0xABu8; 32]).unwrap();
        let mut bytes = shard.to_bytes();
        bytes[3 + CEREMONY_ID_LENGTH] = 0;
        assert_eq!(
            BackupKeyShard::from_bytes(&bytes),
            Err(KeyShardError::InvalidFormat)
        );
    }

    // @internal
    #[test]
    fn backup_key_shard_rejects_index_above_authenticated_count() {
        let shard = BackupKeyShard::new(metadata(2, 3), 1, [0xABu8; 32]).unwrap();
        let mut bytes = shard.to_bytes();
        bytes[3 + CEREMONY_ID_LENGTH] = 4;

        assert_eq!(
            BackupKeyShard::from_bytes(&bytes),
            Err(KeyShardError::InvalidFormat)
        );
    }

    // @internal
    #[test]
    fn backup_key_shard_rejects_bad_length() {
        assert_eq!(
            BackupKeyShard::from_bytes(&[1, 1]),
            Err(KeyShardError::InvalidFormat)
        );
        let mut bytes = BackupKeyShard::new(metadata(2, 3), 1, [0u8; 32])
            .unwrap()
            .to_bytes();
        bytes[0] = SHARD_VERSION + 1;
        assert_eq!(
            BackupKeyShard::from_bytes(&bytes),
            Err(KeyShardError::UnsupportedVersion(SHARD_VERSION + 1))
        );
    }

    // @internal
    #[test]
    fn split_and_reconstruct_backup_key() {
        let key = BackupKey::generate();
        let config = KeyShardConfig::new(2, 3).unwrap();
        let metadata = GuardianBackupMetadata::generate(config);
        let shards = split_backup_key(&key, metadata).unwrap();
        assert_eq!(shards.len(), 3);

        // Any 2 shares reconstruct
        let reconstructed = reconstruct_backup_key(&shards[0..2]).unwrap();
        assert_eq!(reconstructed.as_bytes(), key.as_bytes());

        let reconstructed = reconstruct_backup_key(&shards[1..3]).unwrap();
        assert_eq!(reconstructed.as_bytes(), key.as_bytes());
    }

    // @internal
    #[test]
    fn reconstruct_rejects_mixed_ceremony_metadata() {
        let key = BackupKey::generate();
        let first_metadata = metadata(2, 3);
        let second_metadata =
            GuardianBackupMetadata::new(2, 3, [0x43; CEREMONY_ID_LENGTH]).unwrap();
        let first = split_backup_key(&key, first_metadata).unwrap();
        let second = split_backup_key(&key, second_metadata).unwrap();

        assert!(matches!(
            reconstruct_backup_key(&[first[0].clone(), second[1].clone()]),
            Err(KeyShardError::InvalidFormat)
        ));
    }

    // @internal
    #[test]
    fn seal_and_open_share_for_guardian() {
        let key = BackupKey::generate();
        let config = KeyShardConfig::new(2, 3).unwrap();
        let shards = split_backup_key(&key, GuardianBackupMetadata::generate(config)).unwrap();

        let (guardian_sk, guardian_pk) = guardian_keypair();
        let sealed = seal_share_for_guardian(&shards[0], &guardian_pk).unwrap();
        assert!(!sealed.is_empty());

        let opened = open_share_for_guardian(&sealed, &guardian_sk).unwrap();
        assert_eq!(shards[0], opened);
    }

    // @internal
    #[test]
    fn wrong_guardian_cannot_open_share() {
        let key = BackupKey::generate();
        let config = KeyShardConfig::new(2, 3).unwrap();
        let shards = split_backup_key(&key, GuardianBackupMetadata::generate(config)).unwrap();

        let (_right_sk, right_pk) = guardian_keypair();
        let (wrong_sk, _wrong_pk) = guardian_keypair();

        let sealed = seal_share_for_guardian(&shards[0], &right_pk).unwrap();
        assert!(open_share_for_guardian(&sealed, &wrong_sk).is_err());
    }

    // @internal
    #[test]
    fn default_config_is_2_of_3() {
        let config = KeyShardConfig::DEFAULT;
        assert_eq!(config.threshold(), 2);
        assert_eq!(config.count(), 3);
    }

    // @internal
    #[test]
    fn config_validation_rejects_invalid_params() {
        assert!(KeyShardConfig::new(1, 3).is_err());
        assert!(KeyShardConfig::new(4, 3).is_err());
        assert!(KeyShardConfig::new(2, 11).is_err());
    }

    // @internal
    #[test]
    fn full_flow_end_to_end() {
        // User creates a guardian backup
        let backup_key = BackupKey::generate();
        let config = KeyShardConfig::new(3, 5).unwrap();
        let shards =
            split_backup_key(&backup_key, GuardianBackupMetadata::generate(config)).unwrap();

        // Guardians: each has their own X25519 keypair
        let guardians: Vec<(StaticSecret, PublicKey)> =
            (0..5).map(|_| guardian_keypair()).collect();

        // Seal each share to its guardian
        let sealed_shares: Vec<Vec<u8>> = shards
            .iter()
            .zip(guardians.iter())
            .map(|(shard, (_sk, pk))| seal_share_for_guardian(shard, pk).unwrap())
            .collect();

        // Later, user recovers with any 3 guardians
        let recovered_shards: Vec<BackupKeyShard> = vec![
            open_share_for_guardian(&sealed_shares[0], &guardians[0].0).unwrap(),
            open_share_for_guardian(&sealed_shares[2], &guardians[2].0).unwrap(),
            open_share_for_guardian(&sealed_shares[4], &guardians[4].0).unwrap(),
        ];

        let recovered_key = reconstruct_backup_key(&recovered_shards).unwrap();
        assert_eq!(recovered_key.as_bytes(), backup_key.as_bytes());
    }
}

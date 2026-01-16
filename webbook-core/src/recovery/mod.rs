//! Contact Recovery via Social Vouching
//!
//! Enables users to recover contact relationships after losing all devices
//! through a social vouching mechanism. Users collect in-person vouchers
//! from existing contacts until a threshold is met.
//!
//! # Architecture
//!
//! - `RecoveryClaim`: QR code data claiming ownership of old identity
//! - `RecoveryVoucher`: Signed voucher from a contact
//! - `RecoveryProof`: Collection of vouchers proving identity
//! - `RecoverySettings`: User's recovery preferences

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use thiserror::Error;

use crate::contact::Contact;
use crate::crypto::{PublicKey, Signature, SigningKeyPair};

/// Recovery-related errors.
#[derive(Error, Debug)]
pub enum RecoveryError {
    #[error("Insufficient vouchers: need at least {0}")]
    InsufficientVouchers(u32),

    #[error("Duplicate voucher from same contact")]
    DuplicateVoucher,

    #[error("Voucher has invalid signature")]
    InvalidSignature,

    #[error("Voucher keys don't match proof keys")]
    MismatchedKeys,

    #[error("Recovery proof has expired")]
    ProofExpired,

    #[error("Invalid recovery data format")]
    InvalidFormat,

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Recovery claim shown as QR code.
///
/// Created by a user who lost their device and wants to prove
/// they owned the old identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryClaim {
    claim_type: String,
    old_pk: [u8; 32],
    new_pk: [u8; 32],
    timestamp: u64,
}

impl RecoveryClaim {
    /// Creates a new recovery claim.
    pub fn new(old_pk: &[u8; 32], new_pk: &[u8; 32]) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self {
            claim_type: "recovery_claim".to_string(),
            old_pk: *old_pk,
            new_pk: *new_pk,
            timestamp,
        }
    }

    /// Returns the old (lost) public key.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the new public key.
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }

    /// Returns the claim timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Serializes the claim to bytes (for QR code).
    pub fn to_bytes(&self) -> Vec<u8> {
        // Version byte + old_pk + new_pk + timestamp
        let mut bytes = Vec::with_capacity(1 + 32 + 32 + 8);
        bytes.push(1); // Version 1
        bytes.extend_from_slice(&self.old_pk);
        bytes.extend_from_slice(&self.new_pk);
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes
    }

    /// Deserializes a claim from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecoveryError> {
        // Minimum: version (1) + old_pk (32) + new_pk (32) + timestamp (8) = 73
        if bytes.len() < 73 {
            return Err(RecoveryError::InvalidFormat);
        }

        let version = bytes[0];
        if version != 1 {
            return Err(RecoveryError::InvalidFormat);
        }

        let old_pk: [u8; 32] = bytes[1..33]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;
        let new_pk: [u8; 32] = bytes[33..65]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;
        let timestamp = u64::from_le_bytes(
            bytes[65..73]
                .try_into()
                .map_err(|_| RecoveryError::InvalidFormat)?,
        );

        Ok(Self {
            claim_type: "recovery_claim".to_string(),
            old_pk,
            new_pk,
            timestamp,
        })
    }
}

/// Voucher created by a contact confirming the recovery claim.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryVoucher {
    old_pk: [u8; 32],
    new_pk: [u8; 32],
    voucher_pk: [u8; 32],
    timestamp: u64,
    #[serde_as(as = "[_; 64]")]
    signature: [u8; 64],
}

impl RecoveryVoucher {
    /// Creates a signed voucher.
    pub fn create(old_pk: &[u8; 32], new_pk: &[u8; 32], voucher_keypair: &SigningKeyPair) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let voucher_pk = *voucher_keypair.public_key().as_bytes();

        // Build data to sign
        let data = Self::build_sign_data(old_pk, new_pk, &voucher_pk, timestamp);
        let signature = voucher_keypair.sign(&data);

        Self {
            old_pk: *old_pk,
            new_pk: *new_pk,
            voucher_pk,
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Builds the data to be signed/verified.
    fn build_sign_data(
        old_pk: &[u8; 32],
        new_pk: &[u8; 32],
        voucher_pk: &[u8; 32],
        timestamp: u64,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(32 + 32 + 32 + 8);
        data.extend_from_slice(old_pk);
        data.extend_from_slice(new_pk);
        data.extend_from_slice(voucher_pk);
        data.extend_from_slice(&timestamp.to_le_bytes());
        data
    }

    /// Returns the old (lost) public key.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the new public key.
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }

    /// Returns the voucher's public key.
    pub fn voucher_pk(&self) -> &[u8; 32] {
        &self.voucher_pk
    }

    /// Returns the voucher timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Verifies the voucher signature.
    pub fn verify(&self) -> bool {
        let data = Self::build_sign_data(&self.old_pk, &self.new_pk, &self.voucher_pk, self.timestamp);
        let public_key = PublicKey::from_bytes(self.voucher_pk);
        let signature = Signature::from_bytes(self.signature);
        public_key.verify(&data, &signature)
    }

    /// Serializes the voucher to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Version + old_pk + new_pk + voucher_pk + timestamp + signature
        let mut bytes = Vec::with_capacity(1 + 32 + 32 + 32 + 8 + 64);
        bytes.push(1); // Version 1
        bytes.extend_from_slice(&self.old_pk);
        bytes.extend_from_slice(&self.new_pk);
        bytes.extend_from_slice(&self.voucher_pk);
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.extend_from_slice(&self.signature);
        bytes
    }

    /// Deserializes a voucher from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecoveryError> {
        // Version (1) + old_pk (32) + new_pk (32) + voucher_pk (32) + timestamp (8) + signature (64) = 169
        if bytes.len() < 169 {
            return Err(RecoveryError::InvalidFormat);
        }

        let version = bytes[0];
        if version != 1 {
            return Err(RecoveryError::InvalidFormat);
        }

        let old_pk: [u8; 32] = bytes[1..33]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;
        let new_pk: [u8; 32] = bytes[33..65]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;
        let voucher_pk: [u8; 32] = bytes[65..97]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;
        let timestamp = u64::from_le_bytes(
            bytes[97..105]
                .try_into()
                .map_err(|_| RecoveryError::InvalidFormat)?,
        );
        let signature: [u8; 64] = bytes[105..169]
            .try_into()
            .map_err(|_| RecoveryError::InvalidFormat)?;

        Ok(Self {
            old_pk,
            new_pk,
            voucher_pk,
            timestamp,
            signature,
        })
    }

    /// Sets new_pk for testing purposes (to test tamper detection).
    #[doc(hidden)]
    pub fn set_new_pk_for_testing(&mut self, new_pk: &[u8; 32]) {
        self.new_pk = *new_pk;
    }
}

/// Complete recovery proof with multiple vouchers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryProof {
    old_pk: [u8; 32],
    new_pk: [u8; 32],
    threshold: u32,
    vouchers: Vec<RecoveryVoucher>,
    created_at: u64,
    expires_at: u64,
}

impl RecoveryProof {
    /// Default proof expiration (90 days).
    const DEFAULT_EXPIRY_DAYS: u64 = 90;

    /// Creates a new recovery proof.
    pub fn new(old_pk: &[u8; 32], new_pk: &[u8; 32], threshold: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let expires_at = now + Self::DEFAULT_EXPIRY_DAYS * 24 * 60 * 60;

        Self {
            old_pk: *old_pk,
            new_pk: *new_pk,
            threshold,
            vouchers: Vec::new(),
            created_at: now,
            expires_at,
        }
    }

    /// Returns the old (lost) public key.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the new public key.
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }

    /// Returns the required threshold.
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Returns the number of vouchers.
    pub fn voucher_count(&self) -> usize {
        self.vouchers.len()
    }

    /// Returns the vouchers.
    pub fn vouchers(&self) -> &[RecoveryVoucher] {
        &self.vouchers
    }

    /// Adds a voucher to the proof.
    pub fn add_voucher(&mut self, voucher: RecoveryVoucher) -> Result<(), RecoveryError> {
        // Verify keys match
        if voucher.old_pk() != &self.old_pk || voucher.new_pk() != &self.new_pk {
            return Err(RecoveryError::MismatchedKeys);
        }

        // Verify signature
        if !voucher.verify() {
            return Err(RecoveryError::InvalidSignature);
        }

        // Check for duplicate
        if self
            .vouchers
            .iter()
            .any(|v| v.voucher_pk() == voucher.voucher_pk())
        {
            return Err(RecoveryError::DuplicateVoucher);
        }

        self.vouchers.push(voucher);
        Ok(())
    }

    /// Validates the proof has sufficient valid vouchers.
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.vouchers.len() < self.threshold as usize {
            return Err(RecoveryError::InsufficientVouchers(self.threshold));
        }

        // Check for duplicates
        let mut seen_vouchers = HashSet::new();
        for voucher in &self.vouchers {
            if !seen_vouchers.insert(voucher.voucher_pk) {
                return Err(RecoveryError::DuplicateVoucher);
            }
            if !voucher.verify() {
                return Err(RecoveryError::InvalidSignature);
            }
            if voucher.old_pk != self.old_pk || voucher.new_pk != self.new_pk {
                return Err(RecoveryError::MismatchedKeys);
            }
        }

        Ok(())
    }

    /// Verifies the proof against local contacts and returns confidence level.
    pub fn verify_for_contact(
        &self,
        my_contacts: &[Contact],
        settings: &RecoverySettings,
    ) -> VerificationResult {
        // Find mutual contacts who vouched
        let my_contact_pks: HashSet<_> = my_contacts.iter().map(|c| c.public_key()).collect();

        let mutual_vouchers: Vec<_> = self
            .vouchers
            .iter()
            .filter(|v| my_contact_pks.contains(&v.voucher_pk))
            .collect();

        let mutual_names: Vec<String> = mutual_vouchers
            .iter()
            .filter_map(|v| {
                my_contacts
                    .iter()
                    .find(|c| c.public_key() == &v.voucher_pk)
                    .map(|c| c.display_name().to_string())
            })
            .collect();

        let mutual_count = mutual_vouchers.len() as u32;
        let total = self.vouchers.len();

        if mutual_count >= settings.verification_threshold {
            VerificationResult::HighConfidence {
                mutual_vouchers: mutual_names,
                total_vouchers: total,
            }
        } else if mutual_count > 0 {
            VerificationResult::MediumConfidence {
                mutual_vouchers: mutual_names,
                required: settings.verification_threshold,
                total_vouchers: total,
            }
        } else {
            VerificationResult::LowConfidence {
                total_vouchers: total,
            }
        }
    }

    /// Serializes the proof to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Use bincode for serialization
        bincode::serialize(self).expect("Serialization should not fail")
    }

    /// Deserializes a proof from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecoveryError> {
        bincode::deserialize(bytes)
            .map_err(|e| RecoveryError::SerializationError(e.to_string()))
    }
}

/// Result of verifying a recovery proof against local contacts.
#[derive(Clone, Debug)]
pub enum VerificationResult {
    /// Sufficient mutual contacts vouched (threshold met).
    HighConfidence {
        mutual_vouchers: Vec<String>,
        total_vouchers: usize,
    },

    /// Some mutual contacts vouched, but below threshold.
    MediumConfidence {
        mutual_vouchers: Vec<String>,
        required: u32,
        total_vouchers: usize,
    },

    /// No mutual contacts vouched (isolated contact scenario).
    LowConfidence { total_vouchers: usize },
}

/// User's recovery settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverySettings {
    /// How many vouchers needed to create a valid recovery proof.
    recovery_threshold: u32,

    /// How many mutual contacts must vouch for high-confidence verification.
    verification_threshold: u32,
}

impl Default for RecoverySettings {
    fn default() -> Self {
        Self {
            recovery_threshold: 3,
            verification_threshold: 2,
        }
    }
}

impl RecoverySettings {
    /// Creates new recovery settings.
    pub fn new(recovery_threshold: u32, verification_threshold: u32) -> Self {
        Self {
            recovery_threshold,
            verification_threshold,
        }
    }

    /// Returns the recovery threshold.
    pub fn recovery_threshold(&self) -> u32 {
        self.recovery_threshold
    }

    /// Returns the verification threshold.
    pub fn verification_threshold(&self) -> u32 {
        self.verification_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_claim_roundtrip() {
        let old_pk = [0x01u8; 32];
        let new_pk = [0x02u8; 32];

        let claim = RecoveryClaim::new(&old_pk, &new_pk);
        let bytes = claim.to_bytes();
        let restored = RecoveryClaim::from_bytes(&bytes).unwrap();

        assert_eq!(restored.old_pk(), &old_pk);
        assert_eq!(restored.new_pk(), &new_pk);
    }

    #[test]
    fn test_recovery_voucher_roundtrip() {
        let old_pk = [0x01u8; 32];
        let new_pk = [0x02u8; 32];
        let keypair = SigningKeyPair::generate();

        let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &keypair);
        let bytes = voucher.to_bytes();
        let restored = RecoveryVoucher::from_bytes(&bytes).unwrap();

        assert!(restored.verify());
    }
}

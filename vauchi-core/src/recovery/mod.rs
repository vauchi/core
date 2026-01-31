// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

use std::collections::{HashMap, HashSet};
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

    #[error("Recovery threshold must be at least 1")]
    ThresholdTooLow,

    #[error("Recovery threshold cannot exceed 10")]
    ThresholdTooHigh,

    #[error("Verification threshold must be at least 1")]
    VerificationThresholdTooLow,

    #[error("Verification threshold cannot exceed recovery threshold")]
    VerificationThresholdTooHigh,

    #[error("Recovery claim has expired (older than 48 hours)")]
    ClaimExpired,

    #[error("Cannot vouch for your own recovery (self-vouching not allowed)")]
    SelfVouching,

    #[error("Rate limit exceeded: too many recovery claims in the current window")]
    RateLimitExceeded,
}

// =============================================================================
// Recovery Response
// =============================================================================

/// Response to a recovery claim from another user.
///
/// When a contact presents a recovery claim, the user can:
/// - Accept: acknowledge and update the contact's identity
/// - Reject: refuse the claim (potential impostor)
/// - RemindMeLater: defer the decision for later review
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryResponse {
    /// Accept the recovery claim and update contact identity.
    Accept,
    /// Reject the recovery claim.
    Reject,
    /// Defer the decision until the specified timestamp (Unix seconds).
    RemindMeLater {
        /// Unix timestamp when the user should be reminded.
        remind_at: u64,
    },
}

impl RecoveryResponse {
    /// Returns a string representation suitable for storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            RecoveryResponse::Accept => "accept",
            RecoveryResponse::Reject => "reject",
            RecoveryResponse::RemindMeLater { .. } => "remind_me_later",
        }
    }

    /// Returns the remind_at timestamp if this is a RemindMeLater response.
    pub fn remind_at(&self) -> Option<u64> {
        match self {
            RecoveryResponse::RemindMeLater { remind_at } => Some(*remind_at),
            _ => None,
        }
    }
}

// =============================================================================
// Recovery Rate Limiter
// =============================================================================

/// Rate limiter for recovery claims to prevent abuse.
///
/// Limits the number of recovery claims that can be made within a
/// rolling time window. This prevents Sybil attacks where an attacker
/// floods the system with fraudulent recovery claims.
#[derive(Debug, Clone)]
pub struct RecoveryRateLimiter {
    /// Maximum number of claims allowed per hour.
    pub max_claims_per_hour: u32,
}

impl Default for RecoveryRateLimiter {
    fn default() -> Self {
        Self {
            max_claims_per_hour: 5,
        }
    }
}

impl RecoveryRateLimiter {
    /// Creates a new rate limiter with the specified max claims per hour.
    pub fn new(max_claims_per_hour: u32) -> Self {
        Self {
            max_claims_per_hour,
        }
    }

    /// Checks if a new claim is within the rate limit.
    ///
    /// Returns `true` if the claim is allowed, `false` if the rate limit
    /// would be exceeded.
    ///
    /// # Arguments
    /// * `claim_count` - Number of claims already made in the current window
    /// * `window_start` - Unix timestamp when the current window started
    pub fn check_rate_limit(&self, claim_count: u32, window_start: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // If the window has expired (older than 1 hour), reset
        let window_expired = now.saturating_sub(window_start) >= 3600;

        if window_expired {
            // Window expired, new claim is always allowed
            true
        } else {
            // Within the window, check count
            claim_count < self.max_claims_per_hour
        }
    }
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
    /// Maximum age for a recovery claim (48 hours in seconds).
    pub const MAX_AGE_SECS: u64 = 48 * 60 * 60;

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

    /// Creates a new recovery claim with a specific timestamp.
    /// Used for testing timestamp validation.
    #[doc(hidden)]
    pub fn new_with_timestamp(old_pk: &[u8; 32], new_pk: &[u8; 32], timestamp: u64) -> Self {
        Self {
            claim_type: "recovery_claim".to_string(),
            old_pk: *old_pk,
            new_pk: *new_pk,
            timestamp,
        }
    }

    /// Checks if this claim has expired (older than 48 hours).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        now.saturating_sub(self.timestamp) > Self::MAX_AGE_SECS
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
    /// Creates a signed voucher from a recovery claim.
    ///
    /// Validates that:
    /// - The claim is not expired (less than 48 hours old)
    /// - The voucher is not self-vouching (voucher_pk != new_pk)
    ///
    /// # Errors
    /// - `ClaimExpired` if the claim is older than 48 hours
    /// - `SelfVouching` if the voucher's public key matches the new identity
    pub fn create_from_claim(
        claim: &RecoveryClaim,
        voucher_keypair: &SigningKeyPair,
    ) -> Result<Self, RecoveryError> {
        if claim.is_expired() {
            return Err(RecoveryError::ClaimExpired);
        }

        // Prevent self-vouching
        if voucher_keypair.public_key().as_bytes() == claim.new_pk() {
            return Err(RecoveryError::SelfVouching);
        }

        Ok(Self::create(
            claim.old_pk(),
            claim.new_pk(),
            voucher_keypair,
        ))
    }

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
        let data =
            Self::build_sign_data(&self.old_pk, &self.new_pk, &self.voucher_pk, self.timestamp);
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
    ///
    /// # Errors
    /// - `MismatchedKeys` if voucher keys don't match proof keys
    /// - `InvalidSignature` if voucher signature is invalid
    /// - `DuplicateVoucher` if voucher from same contact already exists
    /// - `SelfVouching` if voucher is from the recovering identity
    pub fn add_voucher(&mut self, voucher: RecoveryVoucher) -> Result<(), RecoveryError> {
        // Verify keys match
        if voucher.old_pk() != &self.old_pk || voucher.new_pk() != &self.new_pk {
            return Err(RecoveryError::MismatchedKeys);
        }

        // Prevent self-vouching (voucher_pk == new_pk)
        if voucher.voucher_pk() == &self.new_pk {
            return Err(RecoveryError::SelfVouching);
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
        bincode::deserialize(bytes).map_err(|e| RecoveryError::SerializationError(e.to_string()))
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
    /// Minimum allowed recovery threshold.
    pub const MIN_RECOVERY_THRESHOLD: u32 = 1;
    /// Maximum allowed recovery threshold.
    pub const MAX_RECOVERY_THRESHOLD: u32 = 10;

    /// Creates new recovery settings with validation.
    ///
    /// # Errors
    /// - `ThresholdTooLow` if recovery_threshold < 1
    /// - `ThresholdTooHigh` if recovery_threshold > 10
    /// - `VerificationThresholdTooLow` if verification_threshold < 1
    /// - `VerificationThresholdTooHigh` if verification_threshold > recovery_threshold
    pub fn new(
        recovery_threshold: u32,
        verification_threshold: u32,
    ) -> Result<Self, RecoveryError> {
        if recovery_threshold < Self::MIN_RECOVERY_THRESHOLD {
            return Err(RecoveryError::ThresholdTooLow);
        }
        if recovery_threshold > Self::MAX_RECOVERY_THRESHOLD {
            return Err(RecoveryError::ThresholdTooHigh);
        }
        if verification_threshold < 1 {
            return Err(RecoveryError::VerificationThresholdTooLow);
        }
        if verification_threshold > recovery_threshold {
            return Err(RecoveryError::VerificationThresholdTooHigh);
        }

        Ok(Self {
            recovery_threshold,
            verification_threshold,
        })
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

// =============================================================================
// Recovery Reminder
// =============================================================================

/// Tracks a dismissed recovery notification for later reminder.
///
/// When a user chooses "Remind Me Later" for a recovery proof,
/// this tracks when they should be reminded again.
#[derive(Debug, Clone)]
pub struct RecoveryReminder {
    /// The old public key of the recovery claim.
    old_pk: [u8; 32],
    /// When the reminder was created/snoozed (Unix timestamp).
    created_at: u64,
    /// Number of days until reminder is due.
    reminder_days: u32,
}

impl RecoveryReminder {
    /// Default reminder period in days.
    pub const DEFAULT_REMINDER_DAYS: u32 = 7;

    /// Creates a new reminder with the default 7-day period.
    pub fn new(old_pk: [u8; 32]) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self {
            old_pk,
            created_at,
            reminder_days: Self::DEFAULT_REMINDER_DAYS,
        }
    }

    /// Creates a new reminder with a custom period.
    pub fn with_days(old_pk: [u8; 32], days: u32) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self {
            old_pk,
            created_at,
            reminder_days: days,
        }
    }

    /// Creates a reminder with a specific timestamp (for testing).
    #[doc(hidden)]
    pub fn new_with_timestamp(old_pk: [u8; 32], created_at: u64, reminder_days: u32) -> Self {
        Self {
            old_pk,
            created_at,
            reminder_days,
        }
    }

    /// Returns the old public key this reminder is for.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the reminder period in days.
    pub fn reminder_days(&self) -> u32 {
        self.reminder_days
    }

    /// Checks if the reminder is due (enough time has passed).
    pub fn is_due(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let elapsed_secs = now.saturating_sub(self.created_at);
        let reminder_secs = u64::from(self.reminder_days) * 24 * 60 * 60;

        elapsed_secs >= reminder_secs
    }

    /// Snoozes the reminder for the specified number of days.
    ///
    /// Resets the created_at timestamp to now.
    pub fn snooze(&mut self, days: u32) {
        self.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        self.reminder_days = days;
    }
}

// =============================================================================
// Recovery Conflict Detection
// =============================================================================

/// Represents a conflicting claim in a recovery conflict.
#[derive(Debug, Clone)]
pub struct ConflictingClaim {
    /// The new public key being claimed.
    new_pk: [u8; 32],
    /// Number of vouchers supporting this claim.
    voucher_count: usize,
}

impl ConflictingClaim {
    /// Returns the new public key.
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }

    /// Returns the number of vouchers.
    pub fn voucher_count(&self) -> usize {
        self.voucher_count
    }
}

/// Represents conflicting recovery claims for the same identity.
///
/// This occurs when multiple recovery proofs exist for the same old_pk
/// but with different new_pks, indicating a potential attack.
#[derive(Debug, Clone)]
pub struct RecoveryConflict {
    /// The old public key that has conflicting claims.
    old_pk: [u8; 32],
    /// The conflicting claims.
    claims: Vec<ConflictingClaim>,
}

impl RecoveryConflict {
    /// Detects if there are conflicting recovery claims.
    ///
    /// Returns `Some(conflict)` if multiple proofs for the same old_pk
    /// have different new_pks. Returns `None` if no conflict exists.
    pub fn detect(proofs: &[RecoveryProof]) -> Option<Self> {
        if proofs.is_empty() {
            return None;
        }

        // Group proofs by old_pk
        let mut by_old_pk: HashMap<[u8; 32], Vec<&RecoveryProof>> = HashMap::new();
        for proof in proofs {
            by_old_pk.entry(*proof.old_pk()).or_default().push(proof);
        }

        // Check each group for conflicts (different new_pks)
        for (old_pk, group) in by_old_pk {
            // Collect unique new_pks with their voucher counts
            let mut new_pks: HashMap<[u8; 32], usize> = HashMap::new();
            for proof in &group {
                let entry = new_pks.entry(*proof.new_pk()).or_insert(0);
                *entry = (*entry).max(proof.voucher_count());
            }

            // Conflict if more than one unique new_pk
            if new_pks.len() > 1 {
                let claims: Vec<ConflictingClaim> = new_pks
                    .into_iter()
                    .map(|(new_pk, voucher_count)| ConflictingClaim {
                        new_pk,
                        voucher_count,
                    })
                    .collect();

                return Some(Self { old_pk, claims });
            }
        }

        None
    }

    /// Returns the old public key that has conflicting claims.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the conflicting claims.
    pub fn claims(&self) -> &[ConflictingClaim] {
        &self.claims
    }
}

// =============================================================================
// Recovery Revocation
// =============================================================================

/// A signed revocation of a recovery proof.
///
/// When a user recovers their old device after having initiated recovery,
/// they can sign a revocation with their old private key to invalidate
/// the recovery proof.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRevocation {
    /// Type marker for QR/serialization.
    revocation_type: String,
    /// The old public key (being recovered from).
    #[serde_as(as = "[_; 32]")]
    old_pk: [u8; 32],
    /// The new public key (that was being recovered to).
    #[serde_as(as = "[_; 32]")]
    new_pk: [u8; 32],
    /// Unix timestamp when revocation was created.
    timestamp: u64,
    /// Signature over (revocation_type || old_pk || new_pk || timestamp).
    #[serde_as(as = "[_; 64]")]
    signature: [u8; 64],
}

impl RecoveryRevocation {
    /// Creates a signed revocation.
    ///
    /// Must be signed with the old private key to prove ownership.
    pub fn create(old_pk: &[u8; 32], new_pk: &[u8; 32], old_keypair: &SigningKeyPair) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let revocation_type = "recovery_revocation".to_string();

        // Build signing message
        let mut msg = Vec::new();
        msg.extend_from_slice(revocation_type.as_bytes());
        msg.extend_from_slice(old_pk);
        msg.extend_from_slice(new_pk);
        msg.extend_from_slice(&timestamp.to_le_bytes());

        let signature = old_keypair.sign(&msg);

        Self {
            revocation_type,
            old_pk: *old_pk,
            new_pk: *new_pk,
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Returns the old public key.
    pub fn old_pk(&self) -> &[u8; 32] {
        &self.old_pk
    }

    /// Returns the new public key (that was being recovered to).
    pub fn new_pk(&self) -> &[u8; 32] {
        &self.new_pk
    }

    /// Returns the timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Verifies the revocation signature using the old public key.
    pub fn verify(&self) -> bool {
        let pk = PublicKey::from_bytes(self.old_pk);
        let sig = Signature::from_bytes(self.signature);

        // Reconstruct signing message
        let mut msg = Vec::new();
        msg.extend_from_slice(self.revocation_type.as_bytes());
        msg.extend_from_slice(&self.old_pk);
        msg.extend_from_slice(&self.new_pk);
        msg.extend_from_slice(&self.timestamp.to_le_bytes());

        pk.verify(&msg, &sig)
    }

    /// Checks if this revocation applies to the given proof.
    ///
    /// Returns true if old_pk and new_pk match.
    pub fn applies_to(&self, proof: &RecoveryProof) -> bool {
        self.old_pk == *proof.old_pk() && self.new_pk == *proof.new_pk()
    }

    /// Serializes the revocation to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Serialization should not fail")
    }

    /// Deserializes a revocation from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecoveryError> {
        bincode::deserialize(bytes).map_err(|e| RecoveryError::SerializationError(e.to_string()))
    }
}

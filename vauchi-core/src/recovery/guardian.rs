// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Guardian token — signed designation proving a contact is a recovery guardian.
//!
//! `GuardianToken` embeds the designator's public key, guardian's public key,
//! a creation timestamp, and an Ed25519 signature with domain separation
//! (ADR-007: `"vauchi-recovery-guardian-v1"`).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use super::RecoveryError;
use crate::crypto::{PublicKey, Signature, SigningKeyPair};

/// Domain separator for guardian token signatures (ADR-007).
const GUARDIAN_DOMAIN: &[u8] = b"vauchi-recovery-guardian-v1";

/// A signed token designating a contact as a recovery guardian.
///
/// The token proves that the designator (identified by `designator_pk`)
/// explicitly named the holder of `guardian_pk` as a recovery guardian.
/// Verification is self-contained: no local contact list is needed.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardianToken {
    /// Designator's Ed25519 public key.
    #[serde_as(as = "[_; 32]")]
    designator_pk: [u8; 32],
    /// Guardian's Ed25519 public key.
    #[serde_as(as = "[_; 32]")]
    guardian_pk: [u8; 32],
    /// Unix timestamp (seconds) when the token was created.
    created_at: u64,
    /// Ed25519 signature over `guardian_pk || GUARDIAN_DOMAIN`.
    #[serde_as(as = "[_; 64]")]
    signature: [u8; 64],
}

impl GuardianToken {
    /// Creates a guardian token signed by `signer`.
    ///
    /// The `designator_pk` is derived from `signer.public_key()`.
    pub fn create(signer: &SigningKeyPair, guardian_pk: PublicKey) -> Self {
        Self::create_with_claimed_pk(signer, signer.public_key(), guardian_pk)
    }

    /// Creates a guardian token where the claimed `designator_pk` may differ
    /// from the actual signer's key.
    ///
    /// This constructor exists for adversarial testing (forgery tests). In
    /// production use `create` instead.
    pub fn create_with_claimed_pk(
        signer: &SigningKeyPair,
        claimed_designator_pk: PublicKey,
        guardian_pk: PublicKey,
    ) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after UNIX_EPOCH")
            .as_secs();

        let msg = build_signed_message(guardian_pk.as_bytes());
        let signature = signer.sign(&msg);

        Self {
            designator_pk: *claimed_designator_pk.as_bytes(),
            guardian_pk: *guardian_pk.as_bytes(),
            created_at,
            signature: *signature.as_bytes(),
        }
    }

    /// Verifies the token's signature against the embedded `designator_pk`.
    ///
    /// Returns `true` if and only if the signature is valid.
    pub fn verify(&self) -> bool {
        let pk = PublicKey::from_bytes(self.designator_pk);
        let sig = Signature::from_bytes(self.signature);
        let msg = build_signed_message(&self.guardian_pk);
        pk.verify(&msg, &sig)
    }

    /// Returns the designator's public key bytes.
    pub fn designator_pk(&self) -> &[u8; 32] {
        &self.designator_pk
    }

    /// Returns the guardian's public key bytes.
    pub fn guardian_pk(&self) -> &[u8; 32] {
        &self.guardian_pk
    }

    /// Returns the creation timestamp in seconds since UNIX epoch.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Returns the raw signature bytes.
    pub fn signature_bytes(&self) -> &[u8; 64] {
        &self.signature
    }

    /// Serializes the token to postcard bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("GuardianToken serialization must not fail")
    }

    /// Deserializes a token from postcard bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecoveryError> {
        postcard::from_bytes(bytes).map_err(|e| RecoveryError::SerializationError(e.to_string()))
    }

    /// Overwrites the embedded `guardian_pk` without re-signing.
    ///
    /// This breaks the signature intentionally and exists only to support
    /// tamper-detection tests.
    #[doc(hidden)]
    pub fn set_guardian_pk_for_testing(&mut self, pk: &[u8; 32]) {
        self.guardian_pk = *pk;
    }
}

/// Builds the message that is signed: `guardian_pk_bytes || GUARDIAN_DOMAIN`.
fn build_signed_message(guardian_pk: &[u8; 32]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(guardian_pk.len() + GUARDIAN_DOMAIN.len());
    msg.extend_from_slice(guardian_pk);
    msg.extend_from_slice(GUARDIAN_DOMAIN);
    msg
}

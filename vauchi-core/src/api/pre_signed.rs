// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pre-signed Shred Messages
//!
//! At identity creation, generates and stores pre-signed messages for
//! emergency (panic) shred. Per DP-3, these messages are **not secret** —
//! stored as an unencrypted file so they remain accessible even after
//! SMK destruction.
//!
//! File location: `{data_dir}/pre_signed_shred.bin`

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::identity::Identity;
use crate::network::{AccountDeletionNotice, DeletionStage};

/// Pre-signed messages for emergency (panic) shred.
///
/// These messages are signed at identity creation and periodically refreshed.
/// They enable panic shred to notify the relay and contacts even after all
/// key material has been destroyed (DP-2: sign-before-destroy).
///
/// Stored unencrypted per DP-3: contains only public keys, timestamps,
/// and signatures — no secret material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSignedShredMessages {
    /// Pre-signed AccountDeletionNotice(Confirmed) for contacts.
    pub deletion_notice: AccountDeletionNotice,
    /// Pre-signed purge request fields for the relay.
    pub purge_request: PreSignedPurgeRequest,
    /// When these messages were last generated/refreshed (unix seconds).
    pub refreshed_at: u64,
}

/// Pre-signed relay purge request fields.
///
/// Contains all v2 fields needed by the relay's authenticated purge handler.
/// The relay will verify the signature over (public_key || purge_token || timestamp).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSignedPurgeRequest {
    /// Signing public key (Ed25519, 32 bytes).
    pub public_key: [u8; 32],
    /// Ed25519 signature over (public_key || purge_token || timestamp).
    pub signature: Vec<u8>,
    /// One-time token for replay prevention (32 bytes).
    pub purge_token: [u8; 32],
    /// Timestamp when the request was signed (unix seconds).
    pub timestamp: u64,
}

/// File name for the pre-signed shred messages.
const PRE_SIGNED_FILE_NAME: &str = "pre_signed_shred.bin";

impl PreSignedShredMessages {
    /// Generates pre-signed shred messages from the given identity.
    ///
    /// Creates:
    /// 1. An AccountDeletionNotice(Confirmed) signed by the identity
    /// 2. A relay PurgeRequest with a random purge_token signed by the identity
    pub fn generate(identity: &Identity) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let public_key = *identity.signing_public_key();

        // Generate deletion notice: sign(public_key || stage_byte || timestamp)
        let deletion_notice = Self::sign_deletion_notice(identity, public_key, now);

        // Generate purge request: sign(public_key || purge_token || timestamp)
        let rng = SystemRandom::new();
        let mut purge_token = [0u8; 32];
        rng.fill(&mut purge_token)
            .expect("System RNG should not fail");

        let purge_request = Self::sign_purge_request(identity, public_key, purge_token, now);

        Self {
            deletion_notice,
            purge_request,
            refreshed_at: now,
        }
    }

    /// Refreshes the pre-signed messages with new timestamps and tokens.
    ///
    /// Should be called on each sync cycle or weekly, whichever comes first.
    /// Regenerates the purge_token for replay prevention.
    pub fn refresh(identity: &Identity) -> Self {
        Self::generate(identity)
    }

    /// Returns the file path for the pre-signed shred messages.
    pub fn file_path(data_dir: &Path) -> PathBuf {
        data_dir.join(PRE_SIGNED_FILE_NAME)
    }

    /// Saves the pre-signed messages to disk (unencrypted per DP-3).
    pub fn save(&self, data_dir: &Path) -> Result<(), PreSignedError> {
        let path = Self::file_path(data_dir);
        let data = postcard::to_allocvec(self)
            .map_err(|e| PreSignedError::SerializationFailed(e.to_string()))?;
        std::fs::write(&path, &data).map_err(|e| PreSignedError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Loads pre-signed messages from disk.
    ///
    /// This must work even after SMK destruction — the file is unencrypted.
    pub fn load(data_dir: &Path) -> Result<Self, PreSignedError> {
        let path = Self::file_path(data_dir);
        let data = std::fs::read(&path).map_err(|e| PreSignedError::IoError(e.to_string()))?;
        postcard::from_bytes(&data)
            .map_err(|e| PreSignedError::DeserializationFailed(e.to_string()))
    }

    /// Signs an AccountDeletionNotice(Confirmed).
    ///
    /// Signed message format: public_key || stage_byte || timestamp_be_bytes
    fn sign_deletion_notice(
        identity: &Identity,
        public_key: [u8; 32],
        timestamp: u64,
    ) -> AccountDeletionNotice {
        let stage = DeletionStage::Confirmed;
        let stage_byte = match stage {
            DeletionStage::Pending => 0u8,
            DeletionStage::Confirmed => 1u8,
            DeletionStage::Cancelled => 2u8,
        };

        let mut message = Vec::with_capacity(32 + 1 + 8);
        message.extend_from_slice(&public_key);
        message.push(stage_byte);
        message.extend_from_slice(&timestamp.to_be_bytes());

        let signature = identity.sign(&message);

        AccountDeletionNotice {
            stage,
            public_key,
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Signs a purge request.
    ///
    /// Signed message format: public_key || purge_token || timestamp_be_bytes
    fn sign_purge_request(
        identity: &Identity,
        public_key: [u8; 32],
        purge_token: [u8; 32],
        timestamp: u64,
    ) -> PreSignedPurgeRequest {
        let mut message = Vec::with_capacity(32 + 32 + 8);
        message.extend_from_slice(&public_key);
        message.extend_from_slice(&purge_token);
        message.extend_from_slice(&timestamp.to_be_bytes());

        let signature = identity.sign(&message);

        PreSignedPurgeRequest {
            public_key,
            signature: signature.as_bytes().to_vec(),
            purge_token,
            timestamp,
        }
    }
}

/// Errors from pre-signed message operations.
#[derive(Debug)]
pub enum PreSignedError {
    /// Failed to serialize pre-signed messages.
    SerializationFailed(String),
    /// Failed to deserialize pre-signed messages.
    DeserializationFailed(String),
    /// File I/O error.
    IoError(String),
}

impl std::fmt::Display for PreSignedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SerializationFailed(e) => write!(f, "Serialization failed: {}", e),
            Self::DeserializationFailed(e) => write!(f, "Deserialization failed: {}", e),
            Self::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for PreSignedError {}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::signature;

    #[test]
    fn test_generate_pre_signed_messages() {
        let identity = Identity::create("Alice");
        let msgs = PreSignedShredMessages::generate(&identity);

        // Deletion notice should be Confirmed stage
        assert_eq!(msgs.deletion_notice.stage, DeletionStage::Confirmed);
        assert_eq!(
            msgs.deletion_notice.public_key,
            *identity.signing_public_key()
        );
        assert!(msgs.deletion_notice.timestamp > 0);

        // Purge request should have matching public key
        assert_eq!(
            msgs.purge_request.public_key,
            *identity.signing_public_key()
        );
        assert!(msgs.purge_request.timestamp > 0);
        assert_eq!(msgs.purge_request.signature.len(), 64);
    }

    #[test]
    fn test_deletion_notice_signature_valid() {
        let identity = Identity::create("Bob");
        let msgs = PreSignedShredMessages::generate(&identity);

        let notice = &msgs.deletion_notice;

        // Reconstruct the signed message
        let stage_byte = 1u8; // Confirmed
        let mut message = Vec::with_capacity(32 + 1 + 8);
        message.extend_from_slice(&notice.public_key);
        message.push(stage_byte);
        message.extend_from_slice(&notice.timestamp.to_be_bytes());

        // Verify using aws-lc-rs directly
        let peer_key = signature::UnparsedPublicKey::new(&signature::ED25519, &notice.public_key);
        peer_key
            .verify(&message, &notice.signature)
            .expect("Deletion notice signature should be valid");
    }

    #[test]
    fn test_purge_request_signature_valid() {
        let identity = Identity::create("Charlie");
        let msgs = PreSignedShredMessages::generate(&identity);

        let purge = &msgs.purge_request;

        // Reconstruct the signed message: public_key || purge_token || timestamp
        let mut message = Vec::with_capacity(32 + 32 + 8);
        message.extend_from_slice(&purge.public_key);
        message.extend_from_slice(&purge.purge_token);
        message.extend_from_slice(&purge.timestamp.to_be_bytes());

        // Verify using aws-lc-rs directly
        let peer_key = signature::UnparsedPublicKey::new(&signature::ED25519, &purge.public_key);
        peer_key
            .verify(&message, &purge.signature)
            .expect("Purge request signature should be valid");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let identity = Identity::create("Dave");
        let msgs = PreSignedShredMessages::generate(&identity);

        let dir = tempfile::tempdir().unwrap();
        msgs.save(dir.path()).unwrap();

        let loaded = PreSignedShredMessages::load(dir.path()).unwrap();
        assert_eq!(
            loaded.deletion_notice.public_key,
            msgs.deletion_notice.public_key
        );
        assert_eq!(
            loaded.deletion_notice.timestamp,
            msgs.deletion_notice.timestamp
        );
        assert_eq!(
            loaded.deletion_notice.signature,
            msgs.deletion_notice.signature
        );
        assert_eq!(
            loaded.purge_request.public_key,
            msgs.purge_request.public_key
        );
        assert_eq!(
            loaded.purge_request.purge_token,
            msgs.purge_request.purge_token
        );
        assert_eq!(loaded.purge_request.signature, msgs.purge_request.signature);
        assert_eq!(loaded.refreshed_at, msgs.refreshed_at);
    }

    #[test]
    fn test_file_readable_without_encryption() {
        let identity = Identity::create("Eve");
        let msgs = PreSignedShredMessages::generate(&identity);

        let dir = tempfile::tempdir().unwrap();
        msgs.save(dir.path()).unwrap();

        // File should exist and be readable as raw bytes
        let path = PreSignedShredMessages::file_path(dir.path());
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.is_empty());

        // Should deserialize from raw bytes
        let loaded: PreSignedShredMessages = postcard::from_bytes(&raw).unwrap();
        assert_eq!(loaded.deletion_notice.stage, DeletionStage::Confirmed);
    }

    #[test]
    fn test_refresh_generates_new_purge_token() {
        let identity = Identity::create("Frank");
        let msgs1 = PreSignedShredMessages::generate(&identity);

        // Small sleep to get different timestamp (if system clock is granular enough)
        let msgs2 = PreSignedShredMessages::refresh(&identity);

        // Purge tokens should be different (random)
        assert_ne!(
            msgs1.purge_request.purge_token,
            msgs2.purge_request.purge_token
        );
    }

    #[test]
    fn test_load_nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = PreSignedShredMessages::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_purge_request_compatible_with_relay_signature_format() {
        // Verify the signature format matches what the relay expects:
        // verify over (public_key || purge_token || timestamp_be_bytes)
        let identity = Identity::create("Grace");
        let msgs = PreSignedShredMessages::generate(&identity);

        let purge = &msgs.purge_request;

        // This is exactly how the relay's verify_purge_signature() constructs the message
        let mut relay_message = Vec::with_capacity(32 + 32 + 8);
        relay_message.extend_from_slice(&purge.public_key);
        relay_message.extend_from_slice(&purge.purge_token);
        relay_message.extend_from_slice(&purge.timestamp.to_be_bytes());

        let peer_key = signature::UnparsedPublicKey::new(&signature::ED25519, &purge.public_key);
        peer_key
            .verify(&relay_message, &purge.signature)
            .expect("Signature must be verifiable using relay's message format");
    }
}

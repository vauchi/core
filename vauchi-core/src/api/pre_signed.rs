// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The relay purge an identity deletion sends, signed when it is sent.
//!
//! The relay accepts a purge only within 60 s of its timestamp, so a purge
//! signed in advance and stored is refused (vauchi/private#899), and an
//! emergency wipe sends nothing (#910). The pre-signed messages earlier
//! versions stored in `{data_dir}/pre_signed_shred.bin` are retired
//! (#911); every shred still removes a file an older install left.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::Identity;

/// A relay purge request: the v2 fields the relay's authenticated purge
/// handler verifies, a signature over
/// `public_key || purge_token || timestamp`. The name predates #911; the
/// request is signed at send time.
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

impl PreSignedPurgeRequest {
    /// A purge signed at `now` with a fresh one-time token.
    pub fn sign_now(identity: &Identity, now: u64) -> Self {
        let public_key = *identity.signing_public_key();
        let purge_token: [u8; 32] = crate::crypto::random_bytes();
        let message = purge_signing_message(&public_key, &purge_token, now, false);
        Self {
            public_key,
            signature: identity.sign(&message).as_bytes().to_vec(),
            purge_token,
            timestamp: now,
        }
    }
}

/// Where versions before #911 stored their pre-signed messages; a shred
/// removes the file, so nothing an older install left outlives it.
pub fn legacy_pre_signed_file(data_dir: &Path) -> PathBuf {
    data_dir.join("pre_signed_shred.bin")
}

/// Appended to a purge's signed message when it also erases the guardian
/// set (#711 D2); a purge without it signs what relays verified before.
const ERASE_GUARDIAN_SET_DOMAIN: &[u8] = b"erase-guardian-set";

/// The bytes a purge request signs:
/// `public_key || purge_token || timestamp_be [|| "erase-guardian-set"]`.
pub(crate) fn purge_signing_message(
    public_key: &[u8; 32],
    purge_token: &[u8; 32],
    timestamp: u64,
    erase_guardian_set: bool,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(32 + 32 + 8 + ERASE_GUARDIAN_SET_DOMAIN.len());
    message.extend_from_slice(public_key);
    message.extend_from_slice(purge_token);
    message.extend_from_slice(&timestamp.to_be_bytes());
    if erase_guardian_set {
        message.extend_from_slice(ERASE_GUARDIAN_SET_DOMAIN);
    }
    message
}

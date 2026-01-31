// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Anonymous Sender Identifiers
//!
//! Provides ephemeral, rotating sender identifiers derived from shared keys.
//! This prevents relay-side correlation of messages to real identities.
//! Anonymous IDs rotate hourly (epoch = unix_timestamp / 3600).

use crate::contact::Contact;
use crate::crypto::HKDF;

/// An anonymous sender identifier that rotates per epoch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AnonymousSender {
    /// The ephemeral anonymous ID (32 bytes, derived via HKDF).
    pub anonymous_id: [u8; 32],
    /// The epoch during which this ID is valid.
    pub epoch: u64,
}

/// Epoch duration in seconds (1 hour).
const EPOCH_DURATION_SECS: u64 = 3600;

impl AnonymousSender {
    /// Computes an anonymous sender ID from a shared key and the current epoch.
    pub fn compute(shared_key: &[u8; 32], epoch: u64) -> Self {
        let anonymous_id = compute_anonymous_id(shared_key, epoch);
        AnonymousSender { anonymous_id, epoch }
    }

    /// Computes an anonymous sender ID for the current epoch.
    pub fn for_current_epoch(shared_key: &[u8; 32]) -> Self {
        let epoch = current_epoch();
        Self::compute(shared_key, epoch)
    }
}

/// Computes an anonymous ID from a shared key and epoch via HKDF.
///
/// The ID is deterministic for the same (key, epoch) pair but changes
/// every epoch, preventing long-term correlation.
pub fn compute_anonymous_id(shared_key: &[u8; 32], epoch: u64) -> [u8; 32] {
    let epoch_bytes = epoch.to_le_bytes();
    HKDF::derive_key(Some(shared_key), &epoch_bytes, b"Vauchi_AnonymousSender")
}

/// Returns the current epoch (unix_timestamp / 3600).
pub fn current_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / EPOCH_DURATION_SECS)
        .unwrap_or(0)
}

/// Resolves an anonymous sender ID to a contact by trying each contact's shared key.
///
/// Returns the matching contact, or None if no contact matches.
pub fn resolve_sender<'a>(
    contacts: &'a [Contact],
    anonymous_id: &[u8; 32],
    epoch: u64,
) -> Option<&'a Contact> {
    // Also check previous epoch to handle clock skew at epoch boundaries
    for contact in contacts {
        let candidate = compute_anonymous_id(contact.shared_key().as_bytes(), epoch);
        if &candidate == anonymous_id {
            return Some(contact);
        }
        // Check previous epoch for boundary tolerance
        if epoch > 0 {
            let prev_candidate = compute_anonymous_id(contact.shared_key().as_bytes(), epoch - 1);
            if &prev_candidate == anonymous_id {
                return Some(contact);
            }
        }
    }
    None
}

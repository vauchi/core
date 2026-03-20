// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Anonymous Sender Identifiers
//!
//! Provides ephemeral, rotating sender identifiers derived from shared keys.
//! This prevents relay-side correlation of messages to real identities.
//! Anonymous IDs rotate hourly (epoch = unix_timestamp / 3600).

use std::collections::HashMap;

use subtle::ConstantTimeEq;

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
        AnonymousSender {
            anonymous_id,
            epoch,
        }
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
    // shared_key is IKM (high-entropy input), epoch embedded in info
    let mut info = b"Vauchi_AnonymousSender_v2".to_vec();
    info.extend_from_slice(&epoch_bytes);
    *HKDF::derive_key(None, shared_key, &info)
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
/// This is O(n) per call. For repeated lookups, use [`SenderIndex`] instead (#104).
pub fn resolve_sender<'a>(
    contacts: &'a [Contact],
    anonymous_id: &[u8; 32],
    epoch: u64,
) -> Option<&'a Contact> {
    // Also check previous epoch to handle clock skew at epoch boundaries
    for contact in contacts {
        let candidate = compute_anonymous_id(contact.shared_key().as_bytes(), epoch);
        if bool::from(candidate.ct_eq(anonymous_id)) {
            return Some(contact);
        }
        // Check previous epoch for boundary tolerance
        if epoch > 0 {
            let prev_candidate = compute_anonymous_id(contact.shared_key().as_bytes(), epoch - 1);
            if bool::from(prev_candidate.ct_eq(anonymous_id)) {
                return Some(contact);
            }
        }
    }
    None
}

/// Resolves a sender_id string (from an EncryptedUpdate) to a contact.
///
/// Tries anonymous resolution first (O(n) scan with epoch tolerance),
/// then falls back to direct contact lookup by ID for backward
/// compatibility with old-format messages that use real identity fingerprints.
///
/// Returns the contact's ID string if resolved, or None.
pub fn resolve_sender_id(contacts: &[Contact], sender_id_hex: &str) -> Option<String> {
    // Try anonymous resolution: decode hex → resolve via shared keys
    if let Ok(anonymous_id_bytes) = hex::decode(sender_id_hex)
        && anonymous_id_bytes.len() == 32
    {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&anonymous_id_bytes);
        let epoch = current_epoch();
        if let Some(contact) = resolve_sender(contacts, &arr, epoch) {
            return Some(contact.id().to_string());
        }
    }

    // Fall back to direct contact lookup (backward compat with old format).
    // Note: uses standard == (not constant-time) because contact IDs are
    // public-key fingerprints, not secrets. An observer cannot learn anything
    // from timing that they don't already know from the wire format.
    contacts
        .iter()
        .find(|c| c.id() == sender_id_hex)
        .map(|c| c.id().to_string())
}

/// Pre-computed index for O(1) anonymous sender ID resolution (#104).
///
/// Maps anonymous IDs (for current + previous epoch) to contact indices.
/// Rebuild when contacts change or on epoch boundary.
pub struct SenderIndex {
    /// Maps anonymous_id → contact_id for both current and previous epochs.
    lookup: HashMap<[u8; 32], String>,
    /// The epoch this index was built for.
    epoch: u64,
}

impl SenderIndex {
    /// Builds the index from a contact list for the given epoch.
    ///
    /// O(n) to build, O(1) per lookup. Includes both current and previous
    /// epoch entries for boundary tolerance.
    pub fn build(contacts: &[Contact], epoch: u64) -> Self {
        let mut lookup = HashMap::with_capacity(contacts.len() * 2);
        for contact in contacts {
            let id_current = compute_anonymous_id(contact.shared_key().as_bytes(), epoch);
            lookup.insert(id_current, contact.id().to_string());
            if epoch > 0 {
                let id_prev = compute_anonymous_id(contact.shared_key().as_bytes(), epoch - 1);
                lookup.insert(id_prev, contact.id().to_string());
            }
        }
        SenderIndex { lookup, epoch }
    }

    /// Resolves an anonymous sender ID to a contact ID in O(1).
    pub fn resolve(&self, anonymous_id: &[u8; 32]) -> Option<&str> {
        self.lookup.get(anonymous_id).map(|s| s.as_str())
    }

    /// Returns true if this index is stale (built for a different epoch).
    pub fn is_stale(&self) -> bool {
        self.epoch != current_epoch()
    }

    /// The epoch this index was built for.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

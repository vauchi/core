// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mailbox Token Derivation
//!
//! Derives daily-rotating tokens used by the relay to route messages
//! without learning anything about participant identities.
//!
//! Two token families:
//! - **Contact tokens**: derived from a shared key negotiated during exchange.
//!   Both parties can independently derive the same token for a given day.
//! - **Self tokens**: derived from the identity master seed. All of a user's
//!   devices derive the same self-token, enabling device-sync message routing.
//!
//! Tokens rotate daily (day_epoch = unix_timestamp / 86400). Clock-skew
//! tolerance is handled by the caller registering both today's and
//! yesterday's tokens.

use crate::crypto::HKDF;

const CONTACT_DOMAIN: &[u8] = b"Vauchi_Mailbox_v1";
const DEVICE_SYNC_DOMAIN: &[u8] = b"Vauchi_DeviceSync_v1";

/// Compute a 32-byte mailbox token for a contact.
///
/// Both parties with the same `shared_key` derive identical tokens for the
/// same `day_epoch`, enabling the relay to route messages without knowing
/// who is talking to whom.
///
/// - `shared_key`: the shared key established during card exchange.
/// - `day_epoch`: current day as Unix timestamp / 86400 (UTC).
pub fn compute_mailbox_token(shared_key: &[u8; 32], day_epoch: u64) -> [u8; 32] {
    let mut info = Vec::with_capacity(CONTACT_DOMAIN.len() + 8);
    info.extend_from_slice(CONTACT_DOMAIN);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    *HKDF::derive_key(None, shared_key, &info)
}

/// Compute a 32-byte self-token for device sync.
///
/// All devices sharing the same `master_seed` derive the same token for a
/// given day, allowing the relay to deliver device-sync messages without a
/// persistent identity identifier.
///
/// - `master_seed`: the identity master seed (32 bytes).
/// - `day_epoch`: current day as Unix timestamp / 86400 (UTC).
pub fn compute_self_token(master_seed: &[u8; 32], day_epoch: u64) -> [u8; 32] {
    let mut info = Vec::with_capacity(DEVICE_SYNC_DOMAIN.len() + 8);
    info.extend_from_slice(DEVICE_SYNC_DOMAIN);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    *HKDF::derive_key(None, master_seed, &info)
}

/// Returns the current day epoch (UTC seconds / 86400).
pub fn current_day_epoch() -> u64 {
    crate::clock::ambient_now_secs() / 86400
}

/// Encode a 32-byte token as a lowercase hex string for wire transmission.
pub fn token_hex(token: &[u8; 32]) -> String {
    hex::encode(token)
}

/// Build padded registration token batches.
///
/// Returns one or more 256-token batches. Most users get exactly one batch.
/// Users with many contacts or long offline periods get 2–3 batches so that
/// no real tokens are silently dropped.
///
/// Each batch is padded to exactly 256 entries with random tokens (so the
/// relay cannot infer the number of real contacts from the registration size)
/// and then shuffled to prevent cross-session positional fingerprinting.
///
/// # Arguments
///
/// - `contact_keys`: slice of shared keys, one per contact.
/// - `master_seed`: identity master seed for self-token derivation.
/// - `current_day`: today's day epoch.
/// - `days_offline`: number of historical days to include for offline catchup.
pub fn batch_register_tokens(
    contact_keys: &[[u8; 32]],
    master_seed: &[u8; 32],
    current_day: u64,
    days_offline: u64,
) -> Vec<Vec<String>> {
    let mut all_tokens = Vec::new();
    let start_day = current_day.saturating_sub(days_offline);

    for day in start_day..=current_day {
        // Self-tokens (current + previous day for clock skew)
        all_tokens.push(token_hex(&compute_self_token(master_seed, day)));
        if day > 0 {
            all_tokens.push(token_hex(&compute_self_token(master_seed, day - 1)));
        }
        for shared_key in contact_keys {
            all_tokens.push(token_hex(&compute_mailbox_token(shared_key, day)));
            if day > 0 {
                all_tokens.push(token_hex(&compute_mailbox_token(shared_key, day - 1)));
            }
        }
    }

    all_tokens.sort_unstable();
    all_tokens.dedup();

    // Split into 256-token batches, each padded and shuffled
    use rand::Rng;
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    let mut batches = Vec::new();

    for chunk in all_tokens.chunks(256) {
        let mut batch = chunk.to_vec();
        // Pad to 256 with random tokens
        while batch.len() < 256 {
            let mut random_token = [0u8; 32];
            rng.fill(&mut random_token);
            batch.push(token_hex(&random_token));
        }
        // Shuffle to prevent positional leakage across sessions
        batch.shuffle(&mut rng);
        batches.push(batch);
    }

    if batches.is_empty() {
        // Edge case: no contacts, no days — generate padding-only batch
        let mut batch = Vec::new();
        while batch.len() < 256 {
            let mut random_token = [0u8; 32];
            rng.fill(&mut random_token);
            batch.push(token_hex(&random_token));
        }
        batches.push(batch);
    }

    batches
}

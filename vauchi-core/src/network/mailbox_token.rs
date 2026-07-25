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
use crate::identifiers::MailboxToken;

const CONTACT_DOMAIN: &[u8] = b"Vauchi_Mailbox_v1";
/// Domain for the F4 device-scoped contact mailbox (ADR-064 Amendment
/// 2026-07-25). Distinct domain so a device-scoped token can never collide
/// with the identity-scoped `CONTACT_DOMAIN` token for the same inputs.
const CONTACT_DEVICE_DOMAIN: &[u8] = b"Vauchi_MailboxDevice_v1";
const DEVICE_SYNC_DOMAIN: &[u8] = b"Vauchi_DeviceSync_v1";
const DEVICE_SYNC_RECIPIENT_DOMAIN: &[u8] = b"Vauchi_DeviceSyncRecipient_v1";

/// Compute a 32-byte mailbox token for one **direction** of a contact channel.
///
/// The token is keyed to the message's RECIPIENT, so each direction of a
/// channel has its OWN mailbox: a party derives its receive token with its own
/// identity key, and a sender derives the peer's token with the peer's identity
/// key. Both still derive the same value for a given direction + day because the
/// `shared_key` is symmetric — but a party NEVER computes (and so never polls)
/// the token it sends to, eliminating the self-echo where each side fetched its
/// own sends back and decrypt-failed them
/// (2026-06-30; previously a single symmetric `H(shared_key, day)` token served
/// both directions).
///
/// - `shared_key`: the shared key established during card exchange.
/// - `recipient_pubkey`: the 32-byte signing public key of the message's
///   recipient (own identity key for a receive/register token; the contact's
///   key for a send token).
/// - `day_epoch`: current day as Unix timestamp / 86400 (UTC).
pub fn compute_mailbox_token(
    shared_key: &[u8; 32],
    recipient_pubkey: &[u8; 32],
    day_epoch: u64,
) -> MailboxToken {
    let mut info = Vec::with_capacity(CONTACT_DOMAIN.len() + 32 + 8);
    info.extend_from_slice(CONTACT_DOMAIN);
    info.extend_from_slice(recipient_pubkey);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    MailboxToken::from_bytes(*HKDF::derive_key(None, shared_key, &info))
}

/// Compute a device-scoped contact mailbox token (F4, ADR-064 Amendment
/// 2026-07-25).
///
/// Like [`compute_mailbox_token`] but folds the recipient DEVICE id into the
/// HKDF input, so each of a contact's devices has an independent opaque
/// mailbox. Relay fetch is destructive; without this, one sibling drains an
/// envelope encrypted for another (the F4 lost-primary root cause). The
/// device id is HKDF input only — never sent to the relay — so the token
/// stays a daily-rotating 32-byte value and introduces no wire-visible
/// per-device correlator (ADR-029/037 preserved), mirroring
/// [`compute_device_sync_token`].
///
/// - `recipient_pubkey`: the recipient's identity signing key (contact's key
///   for a send token; own key for a receive token).
/// - `recipient_device_id`: the target device's 32-byte id.
pub fn compute_device_mailbox_token(
    shared_key: &[u8; 32],
    recipient_pubkey: &[u8; 32],
    recipient_device_id: &[u8; 32],
    day_epoch: u64,
) -> MailboxToken {
    let mut info = Vec::with_capacity(CONTACT_DEVICE_DOMAIN.len() + 32 + 32 + 8);
    info.extend_from_slice(CONTACT_DEVICE_DOMAIN);
    info.extend_from_slice(recipient_pubkey);
    info.extend_from_slice(recipient_device_id);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    MailboxToken::from_bytes(*HKDF::derive_key(None, shared_key, &info))
}

/// Compute a 32-byte self-token for device sync.
///
/// All devices sharing the same `master_seed` derive the same token for a
/// given day, allowing the relay to deliver device-sync messages without a
/// persistent identity identifier.
///
/// - `master_seed`: the identity master seed (32 bytes).
/// - `day_epoch`: current day as Unix timestamp / 86400 (UTC).
pub fn compute_self_token(master_seed: &[u8; 32], day_epoch: u64) -> MailboxToken {
    let mut info = Vec::with_capacity(DEVICE_SYNC_DOMAIN.len() + 8);
    info.extend_from_slice(DEVICE_SYNC_DOMAIN);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    MailboxToken::from_bytes(*HKDF::derive_key(None, master_seed, &info))
}

/// Compute a daily device-sync receive token for one linked device.
///
/// Unlike the legacy identity-wide self token, this includes the target device
/// id. Relay fetch is destructive, so every device needs an independent opaque
/// mailbox; otherwise one sibling can consume an envelope encrypted for
/// another. The device id is inside HKDF input only and is never sent to the
/// relay.
pub fn compute_device_sync_token(
    master_seed: &[u8; 32],
    recipient_device_id: &[u8; 32],
    day_epoch: u64,
) -> MailboxToken {
    let mut info = Vec::with_capacity(DEVICE_SYNC_RECIPIENT_DOMAIN.len() + 32 + 8);
    info.extend_from_slice(DEVICE_SYNC_RECIPIENT_DOMAIN);
    info.extend_from_slice(recipient_device_id);
    info.extend_from_slice(&day_epoch.to_be_bytes());
    MailboxToken::from_bytes(*HKDF::derive_key(None, master_seed, &info))
}

/// Returns the current day epoch (UTC seconds / 86400).
///
/// `now` is the current Unix-epoch seconds — production callers route
/// it from `Vauchi::clock` / `Storage::clock`; tests pin a deterministic
/// value.
pub fn current_day_epoch(now: u64) -> u64 {
    now / 86400
}

/// Encode a 32-byte token as a lowercase hex string for wire transmission.
pub fn token_hex(token: &MailboxToken) -> String {
    hex::encode(token.as_bytes())
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
use crate::rng::SecureRngExt;

pub fn batch_register_tokens(
    rng: &dyn crate::rng::SecureRng,
    contact_keys: &[[u8; 32]],
    own_pubkey: &[u8; 32],
    master_seed: &[u8; 32],
    current_day: u64,
    days_offline: u64,
) -> Vec<Vec<String>> {
    padded_registration_batches(
        rng,
        registration_tokens(
            contact_keys,
            own_pubkey,
            master_seed,
            current_day,
            days_offline,
        ),
    )
}

/// Build contact and legacy shared-device mailbox registration tokens.
fn registration_tokens(
    contact_keys: &[[u8; 32]],
    own_pubkey: &[u8; 32],
    master_seed: &[u8; 32],
    current_day: u64,
    days_offline: u64,
) -> Vec<String> {
    let mut all_tokens = Vec::new();
    let start_day = current_day.saturating_sub(days_offline);

    for day in start_day..=current_day {
        // Self-tokens (current + previous day for clock skew)
        all_tokens.push(token_hex(&compute_self_token(master_seed, day)));
        if day > 0 {
            all_tokens.push(token_hex(&compute_self_token(master_seed, day - 1)));
        }
        // Our own RECEIVE token per contact (keyed to our identity), so the
        // relay routes the peer's sends to us without us also polling the token
        // we send to (directional tokens, 2026-06-30).
        for shared_key in contact_keys {
            all_tokens.push(token_hex(&compute_mailbox_token(
                shared_key, own_pubkey, day,
            )));
            if day > 0 {
                all_tokens.push(token_hex(&compute_mailbox_token(
                    shared_key,
                    own_pubkey,
                    day - 1,
                )));
            }
        }
    }

    all_tokens
}

/// Pad, split, and shuffle real registration tokens without revealing their
/// count or stable position to the relay.
fn padded_registration_batches(
    rng: &dyn crate::rng::SecureRng,
    mut all_tokens: Vec<String>,
) -> Vec<Vec<String>> {
    all_tokens.sort_unstable();
    all_tokens.dedup();

    // Split into 256-token batches, each padded and shuffled
    let mut batches = Vec::new();

    for chunk in all_tokens.chunks(256) {
        let mut batch = chunk.to_vec();
        // Pad to 256 with random tokens
        while batch.len() < 256 {
            let mut random_token = [0u8; 32];
            rng.fill_bytes(&mut random_token);
            batch.push(token_hex(&MailboxToken::from_bytes(random_token)));
        }
        // Shuffle to prevent positional leakage across sessions
        rng.shuffle(&mut batch);
        batches.push(batch);
    }

    if batches.is_empty() {
        // Edge case: no contacts, no days — generate padding-only batch
        let mut batch = Vec::new();
        while batch.len() < 256 {
            let mut random_token = [0u8; 32];
            rng.fill_bytes(&mut random_token);
            batch.push(token_hex(&MailboxToken::from_bytes(random_token)));
        }
        batches.push(batch);
    }

    batches
}

/// Build padded registration batches for contact mailboxes plus one linked
/// device's recipient-specific sync mailbox.
///
/// Device-sync envelopes are encrypted to a single target device and relay
/// fetch consumes blobs, so this mailbox must be unique to that target. The
/// legacy shared self-token remains registered during rollout so envelopes
/// sent before this change can still be received. All tokens stay in the same
/// padded, shuffled registration batches as contact mailboxes.
pub fn batch_register_tokens_with_device_sync(
    rng: &dyn crate::rng::SecureRng,
    contact_keys: &[[u8; 32]],
    own_pubkey: &[u8; 32],
    master_seed: &[u8; 32],
    device_id: &[u8; 32],
    current_day: u64,
    days_offline: u64,
) -> Vec<Vec<String>> {
    let start_day = current_day.saturating_sub(days_offline);
    let mut tokens = registration_tokens(
        contact_keys,
        own_pubkey,
        master_seed,
        current_day,
        days_offline,
    );
    for day in start_day..=current_day {
        tokens.push(token_hex(&compute_device_sync_token(
            master_seed,
            device_id,
            day,
        )));
        if day > 0 {
            tokens.push(token_hex(&compute_device_sync_token(
                master_seed,
                device_id,
                day - 1,
            )));
        }
    }
    padded_registration_batches(rng, tokens)
}

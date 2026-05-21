// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Revocation protocol: canonical signature format and processing logic.
//!
//! When a card owner destroys their identity, they send an `IdentityRevoked`
//! message to each contact. The recipient verifies the Ed25519 signature,
//! then crypto-shreds the CEK, deletes the contact, and records a tombstone.

use super::message::IdentityRevoked;
use crate::storage::Storage;

/// Domain separator for revocation signatures.
/// 25 bytes: `b"vauchi-account-revoked-v1"` (kept for backward compatibility).
pub const REVOCATION_DOMAIN_SEPARATOR: &[u8] = b"vauchi-account-revoked-v1";

/// Computes the canonical byte string for revocation signature.
///
/// Format: `domain_separator(25) || sender_pk(32) || recipient_pk(32) || timestamp_be(8)`
///
/// Total: 97 bytes.
pub fn canonical_revocation_bytes(
    sender_pk: &[u8; 32],
    recipient_pk: &[u8; 32],
    timestamp: u64,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(97);
    bytes.extend_from_slice(REVOCATION_DOMAIN_SEPARATOR);
    bytes.extend_from_slice(sender_pk);
    bytes.extend_from_slice(recipient_pk);
    bytes.extend_from_slice(&timestamp.to_be_bytes());
    bytes
}

/// Processes an incoming revocation signal from a contact.
///
/// Steps:
/// 1. Look up contact by sender_id
/// 2. Reject if no such contact (no-op)
/// 3. Reject stale revocation (timestamp < exchange_timestamp)
/// 4. Verify Ed25519 signature against stored public key
/// 5. Crypto-shred: delete CEK
/// 6. Delete contact row
/// 7. Record tombstone in revoked_senders
pub fn process_revocation(
    revocation: &IdentityRevoked,
    storage: &Storage,
) -> Result<(), crate::storage::StorageError> {
    // 1. Look up contact
    let contact = match storage.load_contact(revocation.sender_id.as_str())? {
        Some(c) => c,
        None => return Ok(()), // No such contact — no-op
    };

    // 2. Reject stale revocation (predates current exchange)
    // Only exchanged contacts have exchange timestamps and public keys
    let Some(exchange_ts) = contact.exchange_timestamp() else {
        return Ok(()); // Imported contact — no revocation possible
    };
    if revocation.timestamp < exchange_ts {
        return Ok(());
    }

    // 3. Verify signature against stored public key
    let Some(public_key) = contact.public_key() else {
        return Ok(()); // Imported contact — no public key to verify
    };
    if !revocation.verify(public_key) {
        return Ok(());
    }

    // 4. Crypto-shred: delete CEK (card becomes permanently unreadable)
    storage.delete_contact_cek(revocation.sender_id.as_str())?;

    // 5. Delete contact row
    storage.delete_contact(revocation.sender_id.as_str())?;

    // 6. Record tombstone (prevents future updates from revoked sender)
    storage.record_revoked_sender(revocation.sender_id.as_str(), revocation.timestamp)?;

    Ok(())
}

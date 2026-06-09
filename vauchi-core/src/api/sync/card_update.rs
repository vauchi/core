// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared Card Update Processing
//!
//! Provides the canonical secure pipeline for processing incoming card
//! updates from contacts. All clients (CLI, TUI, Desktop, Mobile) should
//! use this module instead of implementing their own processing logic.
//!
//! Security checks performed:
//! - Revoked sender rejection
//! - Blocked contact rejection
//! - Ratchet decryption
//! - Versioned payload handling (CEK-wrapped v0x02)
//! - Ed25519 signature verification (sender + recipient binding)
//! - Replay detection via storage nonces
//! - Atomic transaction for all DB writes

use crate::crypto::cek::ContentEncryptionKey;
use crate::crypto::ratchet::RatchetMessage;
use crate::identity::Identity;
use crate::network::anonymous::resolve_sender_id;
use crate::storage::{Storage, StorageError};
use crate::sync::delta::{CardDelta, FieldChange, PAYLOAD_VERSION_CEK, VersionedPayload};

/// Error returned when a single card update fails.
///
/// Individual update failures do not prevent processing of subsequent
/// updates in a batch — they are logged and skipped.
#[derive(Debug)]
#[non_exhaustive]
pub enum CardUpdateError {
    /// Sender has been revoked (tombstone exists).
    SenderRevoked,
    /// Contact not found in storage.
    ContactNotFound,
    /// Contact is blocked.
    ContactBlocked,
    /// No ratchet state available for this contact.
    NoRatchetState,
    /// Failed to deserialize the ratchet message.
    InvalidRatchetMessage,
    /// Ratchet decryption failed (wrong key or corrupted).
    DecryptionFailed,
    /// Payload version or structure is invalid.
    InvalidPayload(String),
    /// CEK decryption of the inner payload failed.
    CekDecryptionFailed,
    /// Delta JSON deserialization failed.
    InvalidDelta,
    /// Ed25519 signature verification failed.
    SignatureInvalid,
    /// Replay attack detected (nonce already seen).
    ReplayDetected,
    /// Delta application failed (invalid field changes).
    DeltaApplicationFailed,
    /// Storage error during transaction.
    Storage(StorageError),
}

impl From<StorageError> for CardUpdateError {
    fn from(e: StorageError) -> Self {
        CardUpdateError::Storage(e)
    }
}

/// Result of processing a batch of card updates.
#[derive(Debug, Default)]
pub struct CardUpdateResult {
    /// Number of updates successfully processed.
    pub processed: u32,
    /// Number of updates skipped due to errors.
    pub skipped: u32,
    /// Display names of contacts whose cards were updated.
    pub updated_names: Vec<String>,
}

/// Processes a batch of incoming card updates with full security checks.
///
/// Each update goes through the complete secure pipeline:
/// 1. Revoked sender check
/// 2. Blocked contact check
/// 3. Ratchet decryption
/// 4. Versioned payload handling (CEK-wrapped or legacy)
/// 5. Signature verification (sender + recipient key binding)
/// 6. Replay detection
/// 7. Delta application
/// 8. Atomic transaction (ratchet state + replay nonce + contact card)
///
/// Individual failures are skipped (logged at caller's discretion) so that
/// one bad update does not prevent processing of the rest.
#[tracing::instrument(level = "debug", skip_all, fields(updates_len = updates.len()), name = "sync.process_card_updates")]
pub fn process_card_updates(
    identity: &Identity,
    storage: &Storage,
    updates: Vec<(String, Vec<u8>)>,
) -> Result<CardUpdateResult, StorageError> {
    let mut result = CardUpdateResult::default();

    // Load contacts once for anonymous sender ID resolution.
    // Anonymous IDs (HKDF-derived, rotating hourly) are resolved to real
    // contact IDs using shared keys. Old-format messages with real identity
    // fingerprints pass through unchanged via the fallback path.
    let contacts = storage.contacts().list_contacts().unwrap_or_default();

    for (sender_id, ciphertext) in updates {
        let resolved_id = resolve_sender_id(&contacts, &sender_id, storage.clock().unix_seconds())
            .unwrap_or(sender_id);
        match process_single_card_update(identity, storage, &resolved_id, &ciphertext) {
            Ok(()) => {
                result.processed += 1;
                // Collect display name for the updated contact
                if let Some(contact) = contacts.iter().find(|c| c.id() == resolved_id) {
                    let name = contact.card().display_name().to_string();
                    if !name.is_empty() && !result.updated_names.contains(&name) {
                        result.updated_names.push(name);
                    }
                }
            }
            Err(_) => result.skipped += 1,
        }
    }

    Ok(result)
}

/// Processes a single card update with all security checks.
///
/// Returns `Ok(())` on success or a `CardUpdateError` describing the failure.
#[tracing::instrument(level = "debug", skip_all, fields(ct_len = ciphertext.len()), name = "sync.process_single_card_update")]
pub fn process_single_card_update(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    ciphertext: &[u8],
) -> Result<(), CardUpdateError> {
    // 1. Reject updates from revoked senders
    if storage.contacts().is_sender_revoked(sender_id)? {
        return Err(CardUpdateError::SenderRevoked);
    }

    // 2. Load contact and reject if blocked
    let mut contact = storage
        .contacts()
        .load_contact(sender_id)?
        .ok_or(CardUpdateError::ContactNotFound)?;

    if contact.is_blocked() {
        return Err(CardUpdateError::ContactBlocked);
    }

    // 3. Load ratchet state and decrypt
    let (mut ratchet, is_initiator) = storage
        .ratchets()
        .load_ratchet_state(sender_id)?
        .ok_or(CardUpdateError::NoRatchetState)?;

    let ratchet_msg: RatchetMessage =
        serde_json::from_slice(ciphertext).map_err(|_| CardUpdateError::InvalidRatchetMessage)?;

    let plaintext = ratchet
        .decrypt(&ratchet_msg)
        .map_err(|_| CardUpdateError::DecryptionFailed)?;

    // 4. Handle versioned payload (CEK-wrapped or legacy)
    let (delta_bytes, new_cek) = decode_versioned_payload(&plaintext)?;

    // 5. Parse delta
    let delta: CardDelta =
        serde_json::from_slice(&delta_bytes).map_err(|_| CardUpdateError::InvalidDelta)?;

    // 6. Verify signature (sender + recipient key binding)
    let sender_pk = contact
        .public_key()
        .ok_or(CardUpdateError::SignatureInvalid)?;
    if !delta.verify(sender_pk, identity.signing_public_key()) {
        return Err(CardUpdateError::SignatureInvalid);
    }

    // 7. Replay detection
    if storage.replay().is_replay_nonce(sender_id, &delta.nonce)? {
        return Err(CardUpdateError::ReplayDetected);
    }

    // 8. Apply delta to contact card
    let mut card = contact.card().clone();
    delta
        .apply(&mut card, storage.clock().unix_seconds())
        .map_err(|_| CardUpdateError::DeltaApplicationFailed)?;
    contact.update_card(card, 0);

    if let Some(cek) = new_cek {
        contact.set_cek(cek);
    }

    // 8b. Clean up orphaned per-field notes for any removed fields.
    //
    // When a contact removes a field from their shared card, any private note
    // the user wrote about that field becomes orphaned in `contact_field_notes`.
    // Best-effort: if the note doesn't exist (or cleanup fails), the card update
    // still proceeds successfully.
    for change in &delta.changes {
        if let FieldChange::Removed { field_id } = change {
            // best-effort orphan cleanup: note may already be absent;
            // either way, the card update below is the source of truth
            #[allow(clippy::let_underscore_must_use)]
            let _ = storage
                .field_notes()
                .delete_contact_field_note(sender_id, field_id);
        }
    }

    // 9. Atomic transaction: ratchet state + replay nonce + contact card (Tracker #159)
    //
    // Ratchet state is saved in the SAME transaction as the contact card update
    // and replay nonce. If any write fails, all are rolled back. This prevents
    // the "ratchet advanced but message lost" crash scenario where the ratchet
    // state advances past a message that was never applied.
    storage.begin_transaction()?;
    let txn_result = (|| -> Result<(), CardUpdateError> {
        storage
            .ratchets()
            .save_ratchet_state(sender_id, &ratchet, is_initiator)?;
        storage
            .replay()
            .save_replay_nonce(sender_id, &delta.nonce, delta.timestamp)?;
        storage.contacts().save_contact(&contact)?;
        Ok(())
    })();

    match txn_result {
        Ok(()) => {
            storage.commit()?;
            Ok(())
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

/// Decodes versioned payload, handling CEK-wrapped (v2) format.
fn decode_versioned_payload(
    plaintext: &[u8],
) -> Result<(Vec<u8>, Option<ContentEncryptionKey>), CardUpdateError> {
    if !plaintext.is_empty() && plaintext[0] == PAYLOAD_VERSION_CEK {
        // Version 0x02: CEK-wrapped payload
        match VersionedPayload::decode(plaintext) {
            Ok(VersionedPayload::CekWrapped(wrapped)) => {
                let cek = ContentEncryptionKey::from_bytes(wrapped.cek);
                let decrypted = cek
                    .decrypt(&wrapped.cek_ciphertext)
                    .map_err(|_| CardUpdateError::CekDecryptionFailed)?;
                Ok((decrypted, Some(cek)))
            }
            Ok(VersionedPayload::ReciprocityConfirm(_)) => {
                // ReciprocityConfirm is not a card delta — handled at caller level
                Err(CardUpdateError::InvalidPayload(
                    "reciprocity confirm not handled here".into(),
                ))
            }
            Err(e) => Err(CardUpdateError::InvalidPayload(e.to_string())),
        }
    } else {
        Err(CardUpdateError::InvalidPayload(
            "unknown payload version".into(),
        ))
    }
}

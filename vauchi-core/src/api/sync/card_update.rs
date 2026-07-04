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
use crate::network::GeoLocation;
use crate::network::anonymous::resolve_sender_id;
use crate::storage::{Storage, StorageError};
use crate::sync::delta::{CardDelta, FieldChange, PAYLOAD_VERSION_CEK, VersionedPayload};
use crate::sync::safety_alert::AlertKind;

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
    /// Delta version is older than the last applied version (downgrade, #42).
    StaleVersion { delta: u32, last: u32 },
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

/// A verified safety alert (emergency/duress) received from a contact.
///
/// Surfaced by the receive pipeline so the caller can dispatch the matching
/// `VauchiEvent` — the alert touched no contact card.
#[derive(Debug, Clone)]
pub struct ReceivedAlert {
    /// Emergency vs duress (drives which event is dispatched).
    pub kind: AlertKind,
    /// The alert message.
    pub message: String,
    /// Unix timestamp when the alert was created.
    pub timestamp: u64,
    /// Optional sender location.
    pub location: Option<GeoLocation>,
}

/// What a single decrypted, verified receive payload turned out to be.
#[derive(Debug)]
pub enum ReceiveOutcome {
    /// A card delta was applied to the contact's card.
    CardDelta,
    /// A safety alert was received (no card change).
    Alert(ReceivedAlert),
    /// A reciprocity confirmation was received (P3 relay-sync, no card change):
    /// the peer proved — with an Ed25519 signature bound to sender + recipient —
    /// that it completed and persisted the exchange. Carries the peer's token
    /// for the app layer to match against the contact's `expected_their_token`
    /// and resolve reciprocity to `Confirmed`.
    ReciprocityConfirm { sender_id: String, token: [u8; 32] },
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
            Ok(ReceiveOutcome::CardDelta) => {
                result.processed += 1;
                // Collect display name for the updated contact
                if let Some(contact) = contacts.iter().find(|c| c.id() == resolved_id) {
                    let name = contact.card().display_name().to_string();
                    if !name.is_empty() && !result.updated_names.contains(&name) {
                        result.updated_names.push(name);
                    }
                }
            }
            // Alerts are surfaced via events on the receive-blob path, not this
            // batch card-update helper; count as processed, no card name.
            Ok(ReceiveOutcome::Alert(_)) => result.processed += 1,
            // Reciprocity confirmations are surfaced via the receive-blob path
            // (like alerts), not this batch card-update helper; count processed.
            Ok(ReceiveOutcome::ReciprocityConfirm { .. }) => result.processed += 1,
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
) -> Result<ReceiveOutcome, CardUpdateError> {
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

    // 3b. Alerts route separately. Emergency/duress alerts reuse the card-update
    //     envelope for wire indistinguishability (ADR-032) but carry no card
    //     delta — a 0x04 payload is verified + surfaced here, before the
    //     card-delta path (2026-07-04-coercion-safety-alerts-never-received).
    if let Ok(VersionedPayload::Alert(alert)) = VersionedPayload::decode(&plaintext) {
        // Verify the sender+recipient signature BEFORE acting on the alert.
        let sender_pk = contact
            .public_key()
            .ok_or(CardUpdateError::SignatureInvalid)?;
        if !alert.verify(sender_pk, identity.signing_public_key()) {
            return Err(CardUpdateError::SignatureInvalid);
        }
        // Replay detection (reuse the card-update nonce store) — a captured
        // alert blob must not be replayable to re-trigger the alert.
        if storage.replay().is_replay_nonce(sender_id, alert.nonce())? {
            return Err(CardUpdateError::ReplayDetected);
        }
        // Persist the advanced ratchet + the nonce atomically. No card is touched.
        storage.begin_transaction()?;
        let alert_txn = (|| -> Result<(), CardUpdateError> {
            storage
                .ratchets()
                .save_ratchet_state(sender_id, &ratchet, is_initiator)?;
            storage
                .replay()
                .save_replay_nonce(sender_id, alert.nonce(), alert.timestamp())?;
            Ok(())
        })();
        match alert_txn {
            Ok(()) => storage.commit()?,
            Err(e) => {
                storage.rollback();
                return Err(e);
            }
        }
        return Ok(ReceiveOutcome::Alert(ReceivedAlert {
            kind: alert.kind(),
            message: alert.message().to_string(),
            timestamp: alert.timestamp(),
            location: alert.location().cloned(),
        }));
    }

    // 3c. Reciprocity confirmations (P3 relay-sync) route separately too: a 0x03
    //     payload proves the peer completed + persisted (design P1/P3). Verify
    //     the sender+recipient Ed25519 signature, advance + save the ratchet,
    //     and surface the peer's token for the app-layer match against
    //     `expected_their_token`. No replay-nonce store: the token is
    //     deterministic and re-confirming is idempotent — a replay can only
    //     re-assert a true Confirmed, never forge one (the token must still
    //     match the expected value, checked at the app layer).
    if let Ok(VersionedPayload::ReciprocityConfirm(confirm)) = VersionedPayload::decode(&plaintext)
    {
        let sender_pk = contact
            .public_key()
            .ok_or(CardUpdateError::SignatureInvalid)?;
        if !confirm.verify(sender_pk, identity.signing_public_key()) {
            return Err(CardUpdateError::SignatureInvalid);
        }
        let token = *confirm.token();
        // Persist the advanced ratchet (decrypt consumed a message). No card,
        // no nonce store — the token match + Confirmed transition are app-layer.
        storage.begin_transaction()?;
        let confirm_txn = storage
            .ratchets()
            .save_ratchet_state(sender_id, &ratchet, is_initiator);
        match confirm_txn {
            Ok(()) => storage.commit()?,
            Err(e) => {
                storage.rollback();
                return Err(e.into());
            }
        }
        return Ok(ReceiveOutcome::ReciprocityConfirm {
            sender_id: sender_id.to_string(),
            token,
        });
    }

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

    // 7b. Reject stale/downgraded delta versions (#42). A withheld or reordered
    // older delta carries an unseen nonce and so passes the replay check above;
    // the version floor is what stops it from downgrading the stored card.
    let last_version = storage
        .contacts()
        .last_delta_version(sender_id)
        .unwrap_or(0);
    if delta.version > 0 && delta.version < last_version {
        return Err(CardUpdateError::StaleVersion {
            delta: delta.version,
            last: last_version,
        });
    }

    // 8. Apply delta to contact card.
    let mut card = contact.card().clone();
    // Heal any pre-upsert duplicate fields before applying, so a single
    // `Removed` fully revokes (2026-06-14-delta-apply-duplicate-fields).
    card.deduplicate_fields();
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
        // Track applied delta version so a later older delta is rejected (#42).
        if delta.version > 0 {
            storage
                .contacts()
                .record_delta_version(sender_id, delta.version)?;
        }
        Ok(())
    })();

    match txn_result {
        Ok(()) => {
            storage.commit()?;
            Ok(ReceiveOutcome::CardDelta)
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
            Ok(VersionedPayload::Alert(_)) => {
                // Safety alerts (emergency/duress) are not card deltas — routed
                // at the caller by version byte, like ReciprocityConfirm
                // (2026-07-04-coercion-safety-alerts-never-received).
                Err(CardUpdateError::InvalidPayload(
                    "safety alert not handled here".into(),
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

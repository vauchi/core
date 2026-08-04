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
use crate::network::anonymous::{current_epoch, resolve_sender_device};
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
    /// A genesis decrypt attempt was rate-limited (ADR-068). Retriable — the
    /// caller must NOT ACK the relay blob; the next fetch retries once the
    /// window resets (plan §REVISION F6).
    GenesisRateLimited,
    /// A signed alert fact already exists for this `(contact, nonce)` with
    /// DIFFERENT bytes — a nonce collision or tamper. Deterministic, so it is
    /// ACKed (retrying cannot resolve it) rather than treated as a transient
    /// storage failure that would loop forever (plan §REVISION F9).
    FactConflict,
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
    /// The alert's signed nonce — its stable identity across re-surfacing.
    pub nonce: [u8; 32],
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
    /// An F4 registry push was received and its broadcast persisted; the
    /// caller must queue the described ack reply (ADR-064 Amendment
    /// 2026-07-25). Never activates our send side — bilaterality.
    RegistryPushReceived(super::registry_handshake::RegistryReplyNeeded),
    /// An F4 registry ack was received (activating on a match, tolerated on
    /// a mismatch); `reply` is the confirming ack to queue when the message
    /// carried the peer's own broadcast as an echo.
    RegistryAckReceived {
        sender_id: String,
        reply: Option<super::registry_handshake::RegistryReplyNeeded>,
    },
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

    let known_device_ids: Vec<[u8; 32]> = contacts
        .iter()
        .flat_map(|contact| {
            storage
                .device()
                .load_contact_active_devices(contact.id())
                .unwrap_or_default()
        })
        .map(|device| device.device_id)
        .collect();

    for (sender_id, ciphertext) in updates {
        let resolved = hex::decode(&sender_id)
            .ok()
            .filter(|bytes| bytes.len() == 32)
            .and_then(|bytes| {
                let mut token = [0u8; 32];
                token.copy_from_slice(&bytes);
                resolve_sender_device(
                    &contacts,
                    &known_device_ids,
                    &token,
                    current_epoch(storage.clock().unix_seconds()),
                )
                .map(|(contact, device_id)| (contact.id().to_string(), device_id))
            });
        let (resolved_id, peer_device_id) = resolved.unwrap_or((sender_id, [0; 32]));
        match process_single_card_update_for_device(
            identity,
            storage,
            &resolved_id,
            &peer_device_id,
            &ciphertext,
        ) {
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
            // Registry-handshake replies are queued on the receive-blob path,
            // not this batch card-update helper; state is already persisted.
            Ok(ReceiveOutcome::RegistryPushReceived(_))
            | Ok(ReceiveOutcome::RegistryAckReceived { .. }) => result.processed += 1,
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
    process_single_card_update_for_device(identity, storage, sender_id, &[0; 32], ciphertext)
}

/// Device-aware receive path used after rotating anonymous-token resolution.
///
/// `peer_device_id` scopes both the ratchet session and the stale-version
/// floor to one of the sender's devices; the legacy all-zero id addresses
/// pre-multi-device peers. This public entry point does not assume the route is
/// authenticated, so any stateless device fallback remains safety-rate-limited.
pub fn process_single_card_update_for_device(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
    ciphertext: &[u8],
) -> Result<ReceiveOutcome, CardUpdateError> {
    process_single_card_update_for_device_with_budget(
        identity,
        storage,
        sender_id,
        peer_device_id,
        ciphertext,
        GenesisBudget::EnforceAndSurface,
    )
}

/// Device-aware receive after an origin hint authenticated the relationship
/// key and bound the selected device to this mailbox token and ciphertext.
///
/// Production callers must only use this after `open_origin_hint` succeeds.
/// The separate entry point makes the safety-budget bypass an enforced call-site
/// decision rather than an assumption inferred from a non-zero device id.
pub fn process_single_card_update_for_authenticated_device(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
    ciphertext: &[u8],
) -> Result<ReceiveOutcome, CardUpdateError> {
    process_single_card_update_for_device_with_budget(
        identity,
        storage,
        sender_id,
        peer_device_id,
        ciphertext,
        GenesisBudget::AuthenticatedDeviceRoute,
    )
}

fn process_single_card_update_for_device_with_budget(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
    ciphertext: &[u8],
    device_route_budget: GenesisBudget,
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
    let existing = storage
        .ratchets()
        .load_ratchet_state_for_device(sender_id, peer_device_id)?;
    let (mut ratchet, is_initiator) = match existing {
        Some(session) => session,
        None if *peer_device_id != [0; 32] => {
            let peer_device = storage
                .device()
                .load_contact_active_devices(sender_id)?
                .into_iter()
                .find(|device| &device.device_id == peer_device_id)
                .ok_or(CardUpdateError::NoRatchetState)?;
            let relationship_key = contact
                .shared_key()
                .ok_or(CardUpdateError::NoRatchetState)?;
            let peer_identity = contact
                .public_key()
                .ok_or(CardUpdateError::NoRatchetState)?;
            crate::exchange::ratchet_bootstrap::bootstrap_device_pair_ratchet(
                relationship_key,
                identity.signing_public_key(),
                identity.device_id(),
                identity.device_info().exchange_keypair(),
                peer_identity,
                peer_device_id,
                &peer_device.exchange_public_key,
            )
            .map_err(|_| CardUpdateError::NoRatchetState)?
        }
        // No `[0;32]` session and no peer registry — a first-contact genesis
        // envelope from a secondary device is the ONLY way to make progress, so
        // a rate-limited attempt retains the blob for retry (ADR-068).
        None => {
            return try_receive_genesis_alert(
                identity,
                storage,
                sender_id,
                &contact,
                ciphertext,
                None,
                CardUpdateError::NoRatchetState,
                GenesisBudget::EnforceAndSurface,
            );
        }
    };

    let ratchet_msg: RatchetMessage =
        serde_json::from_slice(ciphertext).map_err(|_| CardUpdateError::InvalidRatchetMessage)?;

    let plaintext = match ratchet.decrypt(&ratchet_msg) {
        Ok(plaintext) => plaintext,
        // A failed decrypt on the legacy `[0;32]` session may be a genesis
        // message from a sender sibling we hold no session with — try genesis
        // before failing (plan §REVISION F8). We already hold a session here, so
        // a rate-limited attempt on this arm is speculative: fall through to the
        // ordinary decrypt failure (ACKed) rather than retaining the blob, so a
        // burst of ordinary undecryptable traffic cannot pin blobs on the relay.
        Err(_) if *peer_device_id == [0; 32] => {
            return try_receive_genesis_alert(
                identity,
                storage,
                sender_id,
                &contact,
                ciphertext,
                Some(&ratchet_msg),
                CardUpdateError::DecryptionFailed,
                GenesisBudget::EnforceAndFallThrough,
            );
        }
        // An Active responder that has no sending chain deliberately sends a
        // device-scoped genesis card fallback (propagation.rs). Try that
        // authenticated path before treating this as ratchet divergence.
        //
        // Only the exact not-genesis/decrypt failure may repair the established
        // session. Errors after a genesis envelope opened (signature, replay,
        // storage, floor) belong to that envelope and must not tear down an
        // otherwise healthy device ratchet.
        Err(_) => {
            match try_receive_genesis_alert(
                identity,
                storage,
                sender_id,
                &contact,
                ciphertext,
                Some(&ratchet_msg),
                CardUpdateError::DecryptionFailed,
                device_route_budget,
            ) {
                Ok(outcome) => return Ok(outcome),
                // A speculative device route with no authenticated hint keeps
                // the ordinary ACK behavior when the safety budget is
                // exhausted, but budget exhaustion is not ratchet divergence.
                Err(CardUpdateError::GenesisRateLimited) => {
                    return Err(CardUpdateError::DecryptionFailed);
                }
                Err(CardUpdateError::DecryptionFailed) => {
                    super::registry_handshake::repair_device_session(
                        identity,
                        storage,
                        sender_id,
                        peer_device_id,
                    );
                    return Err(CardUpdateError::DecryptionFailed);
                }
                Err(other) => return Err(other),
            }
        }
    };

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
        // Persist the advanced ratchet + the nonce + the alert fact atomically.
        // No card is touched. The fact must commit with the nonce: accepting
        // the alert burns its replay protection, so an alert that exists only
        // in memory is unrecoverable after a crash (delivery-axis findings,
        // 2026-07-21-per-device-ratchet-registry-dormant). The stored bytes
        // are the exact signed wire payload so siblings can re-verify.
        storage.begin_transaction()?;
        let alert_txn = (|| -> Result<(), CardUpdateError> {
            storage.ratchets().save_ratchet_state_for_device(
                sender_id,
                peer_device_id,
                &ratchet,
                is_initiator,
            )?;
            storage
                .replay()
                .save_replay_nonce(sender_id, alert.nonce(), alert.timestamp())?;
            storage.safety_alerts().insert_fact_if_absent(
                sender_id,
                alert.nonce(),
                &plaintext,
                storage.clock().unix_seconds(),
            )?;
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
            nonce: *alert.nonce(),
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
        let confirm_txn = storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            &ratchet,
            is_initiator,
        );
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

    // 3d. F4 registry handshake payloads (0x05/0x06) — persist/ack registry
    //     state inside the receive transaction; the caller queues any reply
    //     (ADR-064 Amendment 2026-07-25).
    if let Ok(VersionedPayload::RegistryPush(push)) = VersionedPayload::decode(&plaintext) {
        return super::registry_handshake::receive_registry_push(
            identity,
            storage,
            sender_id,
            &contact,
            &push,
            &ratchet,
            is_initiator,
            peer_device_id,
        );
    }
    if let Ok(VersionedPayload::RegistryAck(ack)) = VersionedPayload::decode(&plaintext) {
        return super::registry_handshake::receive_registry_ack(
            identity,
            storage,
            sender_id,
            &contact,
            &ack,
            &ratchet,
            is_initiator,
            peer_device_id,
        );
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
    // The floor is per (contact, peer device) because each sender device
    // numbers deltas from its own storage — a per-contact floor would reject a
    // fresh device's legitimate first delta as stale.
    let last_version = storage
        .contacts()
        .last_delta_version_for_device(sender_id, peer_device_id)
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
    let received_at = storage.clock().unix_seconds();
    delta
        .apply(&mut card, received_at)
        .map_err(|_| CardUpdateError::DeltaApplicationFailed)?;
    contact.update_card(
        card,
        source_timestamp_or_received_at(delta.timestamp, received_at),
    );

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
        storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            &ratchet,
            is_initiator,
        )?;
        storage
            .replay()
            .save_replay_nonce(sender_id, &delta.nonce, delta.timestamp)?;
        storage.contacts().save_contact(&contact)?;
        // Track applied delta version so a later older delta is rejected (#42).
        if delta.version > 0 {
            storage.contacts().record_delta_version_for_device(
                sender_id,
                peer_device_id,
                delta.version,
            )?;
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

/// Admission policy for the expensive stateless genesis-open attempt.
enum GenesisBudget {
    /// Unauthenticated cold-start traffic has no other progress path, so
    /// surface the retriable rate-limit error.
    EnforceAndSurface,
    /// A legacy session exists and the attempt is speculative, so fall through
    /// to the ACKable ordinary error when the safety budget is exhausted.
    EnforceAndFallThrough,
    /// A non-zero device route came from the ciphertext-bound, relationship-key
    /// authenticated origin hint. It must not consume the safety-alert budget.
    AuthenticatedDeviceRoute,
}

/// Attempt to receive a stateless genesis envelope (ADR-068, F4).
///
/// A device that holds a contact's `shared_key` but has no established session
/// can send a safety alert sealed into a genesis envelope. This opens it
/// statelessly from `shared_key` + the message header, verifies the inner
/// alert's own signature (possession of `shared_key` is admission to the
/// parser, never authority — plan §REVISION F8), and persists the durable
/// fact + replay nonce + advanced `[0;32]` session in one transaction so an
/// accepted alert survives any crash before surfacing. On any non-genesis or
/// verification failure it returns `fallback`, preserving the original receive
/// error. Unauthenticated legacy routes enforce the durable safety budget;
/// ciphertext-bound origin-hint routes have already authenticated possession of
/// the relationship key and use a separate admission boundary so card traffic
/// cannot exhaust safety-alert capacity.
///
/// MR B does not persist the announced registry (`opened.sender_registry_*`)
/// or create canonical per-device sessions. This is a deliberate deferral, not
/// an omission: production receive routes everything to the `[0;32]` session,
/// so nothing in MR B consumes a learned registry, and persisting it through
/// the existing (destructive, version-monotonic) registry store would risk
/// suppressing a valid alert on a stale/older broadcast. The additive
/// non-destructive merge and per-device routing land together in the routing
/// program that consumes them (plan §REVISION F2/F3/F4). Re-learning the
/// registry from a later genesis is idempotent and cheap.
#[allow(clippy::too_many_arguments)]
fn try_receive_genesis_alert(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    contact: &crate::contact::Contact,
    ciphertext: &[u8],
    pre_parsed: Option<&RatchetMessage>,
    fallback: CardUpdateError,
    budget: GenesisBudget,
) -> Result<ReceiveOutcome, CardUpdateError> {
    let (Some(shared_key), Some(peer_identity)) = (contact.shared_key(), contact.public_key())
    else {
        return Err(fallback);
    };
    // Reuse the caller's already-parsed message where available (the
    // decrypt-failure arm) to avoid a redundant deserialize.
    let parsed;
    let message = match pre_parsed {
        Some(message) => message,
        None => match serde_json::from_slice::<RatchetMessage>(ciphertext) {
            Ok(message) => {
                parsed = message;
                &parsed
            }
            Err(_) => return Err(fallback),
        },
    };

    // On legacy routes, rate-limit BEFORE deriving keys: a genesis open derives
    // ratchet keys from shared_key ahead of any signature check (F6). A
    // non-zero route is admitted only after the origin hint's AEAD authenticates
    // the relationship key and binds this exact ciphertext + mailbox token; it
    // therefore must not consume the distinct safety-alert budget.
    if !matches!(budget, GenesisBudget::AuthenticatedDeviceRoute)
        && !storage.genesis_limits().consume_decrypt_budget(sender_id)?
    {
        return match budget {
            GenesisBudget::EnforceAndSurface => Err(CardUpdateError::GenesisRateLimited),
            GenesisBudget::EnforceAndFallThrough => Err(fallback),
            GenesisBudget::AuthenticatedDeviceRoute => unreachable!(),
        };
    }

    let Ok(opened) = crate::exchange::genesis::GenesisEnvelope::open(
        shared_key,
        peer_identity,
        identity.signing_public_key(),
        message,
    ) else {
        return Err(fallback);
    };

    // F4 slice 4b: genesis-sealed handshake payloads route to the registry
    // handlers. Their authority is the envelope's identity-signed header
    // broadcast (verified at persist); no alert semantics, no [0;32]
    // session persist, no replay burn (idempotent, rate-limited above).
    match VersionedPayload::decode(&opened.inner_payload) {
        Ok(VersionedPayload::RegistryPush(push)) => {
            return super::registry_handshake::receive_genesis_registry_push(
                identity,
                storage,
                sender_id,
                contact,
                opened.sender_device_id,
                &opened.sender_registry_broadcast_json,
                &push,
            );
        }
        Ok(VersionedPayload::RegistryAck(ack)) => {
            return super::registry_handshake::receive_genesis_registry_ack(
                identity,
                storage,
                sender_id,
                contact,
                opened.sender_device_id,
                &opened.sender_registry_broadcast_json,
                &ack,
            );
        }
        _ => {}
    }

    // Genesis-sealed CARD delta — the responder-side card fan-out fallback
    // (propagation.rs). The sender is on the responder side of the device-pair
    // ratchet and cannot send a plain ratchet card, so it seals the delta into
    // this stateless envelope. Apply it exactly like the ratchet card path
    // (verify the delta's own sender→recipient signature; shared_key possession
    // is parser admission, never authority), persisting the advanced [0;32]
    // session under the same cold-start reseat guard as the alert path below.
    if opened.inner_payload.first() == Some(&PAYLOAD_VERSION_CEK) {
        return receive_genesis_card_delta(
            identity,
            storage,
            sender_id,
            contact,
            peer_identity,
            &opened,
        );
    }

    // Authority check: the inner payload must be a safety alert whose own
    // sender→recipient signature verifies, independent of the envelope.
    let alert = match VersionedPayload::decode(&opened.inner_payload) {
        Ok(VersionedPayload::Alert(alert)) => alert,
        _ => return Err(fallback),
    };
    if !alert.verify(peer_identity, identity.signing_public_key()) {
        return Err(CardUpdateError::SignatureInvalid);
    }
    if storage.replay().is_replay_nonce(sender_id, alert.nonce())? {
        return Err(CardUpdateError::ReplayDetected);
    }

    // Durable, atomic: accepting the alert burns its replay nonce, so the fact
    // must commit with it (delivery-axis findings). The advanced responder
    // ratchet persists under `[0;32]` ONLY on true cold start — re-seating
    // over an existing session would silently sever the exchanging device's
    // chain, and that device is the relationship's sole card mediator
    // (ADR-064 Amendment 2026-07-24, guarded invariant 1;
    // problems/2026-07-24-genesis-reseat-severs-live-primary-channel).
    // Checked inside the transaction so the decision and the persist are
    // atomic. A skipped re-seat is counted (PII-free) to size F4 urgency.
    let ratchet = opened.advanced_ratchet;
    storage.begin_transaction()?;
    let genesis_txn = (|| -> Result<(), CardUpdateError> {
        let cold_start = storage
            .ratchets()
            .load_ratchet_state_for_device(sender_id, &[0; 32])?
            .is_none();
        if cold_start {
            storage
                .ratchets()
                .save_ratchet_state_for_device(sender_id, &[0; 32], &ratchet, false)?;
        } else {
            storage.genesis_limits().record_reseat_skip()?;
        }
        storage
            .replay()
            .save_replay_nonce(sender_id, alert.nonce(), alert.timestamp())?;
        // F4 slice 4a: persist the carried identity-signed registry
        // (additive, monotonic-guarded — ADR-068 req 6) so this receiver can
        // afterwards address the sender identity's devices. Tolerates a
        // held-or-newer version; a broadcast the store rejects otherwise is
        // not fatal to the alert (the alert's own signature already
        // verified) — registry seeding is opportunistic here.
        if let Ok(broadcast_text) = std::str::from_utf8(&opened.sender_registry_broadcast_json)
            && let Ok(broadcast) = crate::identity::RegistryBroadcast::from_json(broadcast_text)
        {
            let persisted = storage.device().save_contact_device_registry(
                sender_id,
                &broadcast,
                peer_identity,
                u64::MAX,
            );
            let held_version = match persisted {
                Ok(()) => Some(broadcast.version()),
                Err(_) => storage
                    .device()
                    .load_contact_device_registry(sender_id)?
                    .filter(|stored| stored.version() >= broadcast.version())
                    .map(|stored| stored.version()),
            };
            if let Some(version) = held_version {
                let mut tracker = storage
                    .registry_activation()
                    .load_activation(sender_id)?
                    .unwrap_or_default();
                tracker.record_peer_registry(version);
                storage
                    .registry_activation()
                    .save_activation(sender_id, &tracker)?;
            }
        }
        match storage.safety_alerts().insert_or_compare_fact(
            sender_id,
            alert.nonce(),
            &opened.inner_payload,
            storage.clock().unix_seconds(),
        ) {
            Ok(_) => Ok(()),
            // A same-nonce/different-bytes conflict is deterministic — surface
            // it as an ACKable rejection, not a transient storage failure that
            // the caller would retry against forever (F9).
            Err(StorageError::InvalidData(_)) => Err(CardUpdateError::FactConflict),
            Err(e) => Err(CardUpdateError::Storage(e)),
        }
    })();
    match genesis_txn {
        // Roll back on commit failure too — a failed COMMIT can leave the
        // SQLite transaction open, wedging every later `BEGIN IMMEDIATE`.
        Ok(()) => {
            if let Err(e) = storage.commit() {
                storage.rollback();
                return Err(CardUpdateError::Storage(e));
            }
            super::registry_handshake::journal_handshake_state_for_siblings(
                identity, storage, sender_id,
            );
        }
        Err(e) => {
            storage.rollback();
            return Err(e);
        }
    }

    Ok(ReceiveOutcome::Alert(ReceivedAlert {
        kind: alert.kind(),
        message: alert.message().to_string(),
        timestamp: alert.timestamp(),
        location: alert.location().cloned(),
        nonce: *alert.nonce(),
    }))
}

/// Apply a genesis-sealed CARD delta (the responder-side card fan-out
/// fallback, `propagation.rs`). Verification and application are identical to
/// the ratchet card path (§4-9 of `process_single_card_update_for_device`):
/// possession of `shared_key` admits the payload to the parser but is NEVER
/// authority — the delta's own sender→recipient signature is the authority.
/// The advanced `[0;32]` session persists ONLY on true cold start: re-seating
/// over a live session would silently sever the exchanging device's chain
/// (`problems/2026-07-24-genesis-reseat-severs-live-primary-channel`), the same
/// guard the genesis alert path enforces.
fn receive_genesis_card_delta(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    contact: &crate::contact::Contact,
    peer_identity: &[u8; 32],
    opened: &crate::exchange::genesis::OpenedGenesis,
) -> Result<ReceiveOutcome, CardUpdateError> {
    let (delta_bytes, new_cek) = decode_versioned_payload(&opened.inner_payload)?;
    let delta: CardDelta =
        serde_json::from_slice(&delta_bytes).map_err(|_| CardUpdateError::InvalidDelta)?;

    // Authority is the delta's own signature, never envelope possession.
    if !delta.verify(peer_identity, identity.signing_public_key()) {
        return Err(CardUpdateError::SignatureInvalid);
    }
    if storage.replay().is_replay_nonce(sender_id, &delta.nonce)? {
        return Err(CardUpdateError::ReplayDetected);
    }
    // The [0;32] key identifies only the genesis cold-start ratchet channel.
    // Keying the floor by the envelope-authenticated origin device keeps
    // sibling version streams independent.
    let last_version = storage
        .contacts()
        .last_delta_version_for_device(sender_id, &opened.sender_device_id)
        .unwrap_or(0);
    if delta.version > 0 && delta.version < last_version {
        return Err(CardUpdateError::StaleVersion {
            delta: delta.version,
            last: last_version,
        });
    }

    let mut card = contact.card().clone();
    card.deduplicate_fields();
    let received_at = storage.clock().unix_seconds();
    delta
        .apply(&mut card, received_at)
        .map_err(|_| CardUpdateError::DeltaApplicationFailed)?;
    let mut updated = contact.clone();
    updated.update_card(
        card,
        source_timestamp_or_received_at(delta.timestamp, received_at),
    );
    if let Some(cek) = new_cek {
        updated.set_cek(cek);
    }

    storage.begin_transaction()?;
    let txn = (|| -> Result<(), CardUpdateError> {
        let cold_start = storage
            .ratchets()
            .load_ratchet_state_for_device(sender_id, &[0; 32])?
            .is_none();
        if cold_start {
            storage.ratchets().save_ratchet_state_for_device(
                sender_id,
                &[0; 32],
                &opened.advanced_ratchet,
                false,
            )?;
        } else {
            storage.genesis_limits().record_reseat_skip()?;
        }
        storage
            .replay()
            .save_replay_nonce(sender_id, &delta.nonce, delta.timestamp)?;
        storage.contacts().save_contact(&updated)?;
        if delta.version > 0 {
            storage.contacts().record_delta_version_for_device(
                sender_id,
                &opened.sender_device_id,
                delta.version,
            )?;
        }
        Ok(())
    })();
    match txn {
        Ok(()) => {
            storage.commit()?;
            for change in &delta.changes {
                if let FieldChange::Removed { field_id } = change {
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = storage
                        .field_notes()
                        .delete_contact_field_note(sender_id, field_id);
                }
            }
            Ok(ReceiveOutcome::CardDelta)
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

/// Preserve an authenticated sender timestamp for sibling-device ordering when
/// it is within the existing sync clock-skew policy. Legacy zero timestamps and
/// implausible future values fall back to the local receive time so they cannot
/// become permanent last-write-wins fences.
fn source_timestamp_or_received_at(source_timestamp: u64, received_at: u64) -> u64 {
    if crate::sync::validate_timestamp(source_timestamp, received_at) {
        source_timestamp
    } else {
        received_at
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
                let cek = ContentEncryptionKey::try_from_bytes(wrapped.cek)
                    .map_err(|_| CardUpdateError::CekDecryptionFailed)?;
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
            Ok(VersionedPayload::RegistryPush(_)) | Ok(VersionedPayload::RegistryAck(_)) => {
                // F4 registry handshake payloads are not card deltas — routed
                // at the caller by version byte, like ReciprocityConfirm.
                Err(CardUpdateError::InvalidPayload(
                    "registry handshake not handled here".into(),
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

/// Map a received safety alert to its recipient-side event. Emergency and
/// duress get distinct events so the recipient can respond appropriately; the
/// distinction only exists here (post-decryption), never on the wire
/// (ADR-032). Lives beside `ReceivedAlert` (not in transport code) because
/// surfacing from durable facts must work without the `network-http` feature.
pub(crate) fn alert_event(
    contact_id: String,
    alert: &ReceivedAlert,
) -> crate::api::events::VauchiEvent {
    use crate::api::events::VauchiEvent;
    let message = alert.message.clone();
    let timestamp = alert.timestamp;
    let location = alert.location.as_ref().map(|l| (l.latitude, l.longitude));
    match alert.kind {
        AlertKind::Emergency => VauchiEvent::EmergencyAlertReceived {
            contact_id,
            message,
            timestamp,
            location,
            alert_nonce: alert.nonce,
        },
        AlertKind::Duress => VauchiEvent::DuressAlertReceived {
            contact_id,
            message,
            timestamp,
            location,
            alert_nonce: alert.nonce,
        },
    }
}

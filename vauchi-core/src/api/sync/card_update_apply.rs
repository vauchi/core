// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The receive path after a blob opened (vauchi/private#1093 slice 7): the
//! foreground calls it after its decrypt, the merge at the unlock for what a
//! locked run opened.

use super::*;

/// Everything after a blob opened: alerts, reciprocity confirmations, the
/// ratchet-only kinds, registry pushes and acks, and card deltas, each
/// saved with `ratchet` in one transaction. The merge at the unlock calls
/// it for what a locked run opened (vauchi/private#1093 slice 7).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_opened_update(
    identity: &Identity,
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
    mut contact: Contact,
    ratchet: &DoubleRatchetState,
    is_initiator: bool,
    plaintext: &[u8],
) -> Result<ReceiveOutcome, CardUpdateError> {
    // 3b. Alerts route separately. Emergency alerts reuse the card-update
    //     envelope for wire indistinguishability (ADR-036) but carry no card
    //     delta — a 0x04 payload is verified + surfaced here, before the
    //     card-delta path (2026-07-04-coercion-safety-alerts-never-received).
    if let Ok(VersionedPayload::Alert(alert)) = VersionedPayload::decode(plaintext) {
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
                ratchet,
                is_initiator,
            )?;
            storage
                .replay()
                .save_replay_nonce(sender_id, alert.nonce(), alert.timestamp())?;
            storage.safety_alerts().insert_fact_if_absent(
                sender_id,
                alert.nonce(),
                plaintext,
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
    if let Ok(VersionedPayload::ReciprocityConfirm(confirm)) = VersionedPayload::decode(plaintext) {
        let sender_pk = contact
            .public_key()
            .ok_or(CardUpdateError::SignatureInvalid)?;
        if !confirm.verify(sender_pk, identity.signing_public_key()) {
            return Err(CardUpdateError::SignatureInvalid);
        }
        let token = *confirm.token();
        let sender_waiting = confirm.sender_waiting();
        // Persist the advanced ratchet (decrypt consumed a message). No card,
        // no nonce store — the token match + Confirmed transition are app-layer.
        storage.begin_transaction()?;
        let confirm_txn = storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            ratchet,
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
            sender_waiting,
        });
    }

    // 3c'. The kinds that apply no card change (`ratchet_only`).
    if let Some(outcome) = ratchet_only::outcome_of(VersionedPayload::decode(plaintext)) {
        return ratchet_only::receive(
            storage,
            sender_id,
            peer_device_id,
            ratchet,
            is_initiator,
            outcome,
        );
    }

    // 3d. F4 registry handshake payloads (0x05/0x06) — persist/ack registry
    //     state inside the receive transaction; the caller queues any reply
    //     (ADR-064 Amendment 2026-07-25).
    if let Ok(VersionedPayload::RegistryPush(push)) = VersionedPayload::decode(plaintext) {
        return super::super::registry_handshake::receive_registry_push(
            identity,
            storage,
            sender_id,
            &contact,
            &push,
            ratchet,
            is_initiator,
            peer_device_id,
        );
    }
    if let Ok(VersionedPayload::RegistryAck(ack)) = VersionedPayload::decode(plaintext) {
        return super::super::registry_handshake::receive_registry_ack(
            identity,
            storage,
            sender_id,
            &contact,
            &ack,
            ratchet,
            is_initiator,
            peer_device_id,
        );
    }

    // 4. Handle versioned payload (CEK-wrapped only)
    let (delta_bytes, new_cek) = decode_versioned_payload(plaintext)?;

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

    verify_delta_device_signature(storage, sender_id, &delta)?;

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

    // 9. Atomic transaction: ratchet state + replay nonce + contact card (Tracker #159)
    //
    // Ratchet state is saved in the SAME transaction as the contact card update
    // and replay nonce. If any write fails, all are rolled back. This prevents
    // the "ratchet advanced but message lost" crash scenario where the ratchet
    // state advances past a message that was never applied.
    storage.begin_transaction()?;
    let txn_result = (|| -> Result<Vec<String>, CardUpdateError> {
        let ordered = ordering::apply(
            storage,
            sender_id,
            contact.card().clone(),
            &delta,
            peer_device_id,
        )?;
        contact.update_card(
            ordered.card,
            source_timestamp_or_received_at(delta.timestamp, storage.clock().unix_seconds()),
        );
        if let Some(cek) = new_cek {
            contact.set_cek(cek);
        }
        storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            ratchet,
            is_initiator,
        )?;
        storage
            .replay()
            .save_replay_nonce(sender_id, &delta.nonce, delta.timestamp)?;
        storage.contacts().save_contact(&contact)?;
        storage
            .contacts()
            .save_card_ordering(sender_id, &ordered.state)?;
        // Track applied delta version so a later older delta is rejected (#42).
        if delta.version > 0 {
            storage.contacts().record_delta_version_for_device(
                sender_id,
                peer_device_id,
                delta.version,
            )?;
        }
        Ok(ordered.removed_fields)
    })();

    match txn_result {
        Ok(removed_fields) => {
            if let Err(error) = storage.commit() {
                storage.rollback();
                return Err(error.into());
            }
            cleanup_removed_field_notes(storage, sender_id, &removed_fields);
            Ok(ReceiveOutcome::CardDelta)
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 handshake receive arms — RegistryPush (0x05) / RegistryAck (0x06)
//! arriving on the ratcheted contact channel (ADR-064 Amendment 2026-07-25).
//!
//! Both arms run inside the same transaction as the advanced-ratchet
//! persist, so a storage failure leaves the message undelivered rather than
//! half-applied. Receiving a push never activates our send side — only an
//! ack answering OUR outstanding push does (bilaterality). Persisting a
//! carried broadcast uses the monotonic store guard with an unbounded age
//! window like the device-link path: deliveries to an offline peer can be
//! arbitrarily delayed, and the signature + version monotonicity are the
//! security boundary, not the timestamp.

use super::card_update::{CardUpdateError, ReceiveOutcome};
use crate::contact::Contact;
use crate::crypto::ratchet::DoubleRatchetState;
use crate::identity::RegistryBroadcast;
use crate::storage::Storage;
use crate::sync::registry_activation::{
    ActivationTracker, RegistryAckPayload, RegistryPushPayload,
};

/// What the caller must send back so the peer's handshake can progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryReplyNeeded {
    /// Contact the reply goes to.
    pub sender_id: String,
    /// The registry version we now hold for them (what the reply acks).
    pub acked_version: u64,
    /// Handshake correlation nonce, echoed back verbatim.
    pub push_nonce: [u8; 32],
}

/// Persist a carried broadcast for `sender_id`, tolerating idempotent
/// re-pushes: a version we already hold (or older) acks the held version.
/// Anything the store rejects that is NOT already covered by a held newer
/// version fails closed.
fn persist_carried_broadcast(
    storage: &Storage,
    sender_id: &str,
    contact: &Contact,
    broadcast_json: &[u8],
) -> Result<u64, CardUpdateError> {
    let sender_pk = contact
        .public_key()
        .ok_or(CardUpdateError::SignatureInvalid)?;
    let text = std::str::from_utf8(broadcast_json)
        .map_err(|_| CardUpdateError::InvalidPayload("registry broadcast not UTF-8".into()))?;
    let broadcast = RegistryBroadcast::from_json(text)
        .map_err(|_| CardUpdateError::InvalidPayload("registry broadcast malformed".into()))?;

    match storage
        .device()
        .save_contact_device_registry(sender_id, &broadcast, sender_pk, u64::MAX)
    {
        Ok(()) => Ok(broadcast.version()),
        Err(save_error) => {
            let held = storage
                .device()
                .load_contact_device_registry(sender_id)
                .map_err(CardUpdateError::from)?;
            match held {
                Some(stored) if stored.version() >= broadcast.version() => Ok(stored.version()),
                _ => Err(CardUpdateError::InvalidPayload(format!(
                    "registry broadcast rejected: {save_error}"
                ))),
            }
        }
    }
}

fn load_tracker(storage: &Storage, sender_id: &str) -> Result<ActivationTracker, CardUpdateError> {
    Ok(storage
        .registry_activation()
        .load_activation(sender_id)?
        .unwrap_or_default())
}

/// Handle a received RegistryPush inside the receive transaction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_registry_push(
    storage: &Storage,
    sender_id: &str,
    contact: &Contact,
    push: &RegistryPushPayload,
    ratchet: &DoubleRatchetState,
    is_initiator: bool,
    peer_device_id: &[u8; 32],
) -> Result<ReceiveOutcome, CardUpdateError> {
    storage.begin_transaction()?;
    let txn = (|| -> Result<u64, CardUpdateError> {
        let acked_version =
            persist_carried_broadcast(storage, sender_id, contact, push.broadcast_json())?;
        let mut tracker = load_tracker(storage, sender_id)?;
        tracker.record_peer_registry(acked_version);
        storage
            .registry_activation()
            .save_activation(sender_id, &tracker)?;
        storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            ratchet,
            is_initiator,
        )?;
        Ok(acked_version)
    })();
    match txn {
        Ok(acked_version) => {
            storage.commit()?;
            Ok(ReceiveOutcome::RegistryPushReceived(RegistryReplyNeeded {
                sender_id: sender_id.to_string(),
                acked_version,
                push_nonce: *push.push_nonce(),
            }))
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

/// Handle a received RegistryAck inside the receive transaction.
///
/// A mismatched ack (stale nonce/version — e.g. crossing a registry change
/// in flight) is tolerated without state change: the message is valid and
/// ACKable, there is just nothing to record.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_registry_ack(
    storage: &Storage,
    sender_id: &str,
    contact: &Contact,
    ack: &RegistryAckPayload,
    ratchet: &DoubleRatchetState,
    is_initiator: bool,
    peer_device_id: &[u8; 32],
) -> Result<ReceiveOutcome, CardUpdateError> {
    storage.begin_transaction()?;
    let txn = (|| -> Result<Option<RegistryReplyNeeded>, CardUpdateError> {
        let mut tracker = load_tracker(storage, sender_id)?;
        if tracker
            .record_ack(ack.push_nonce(), ack.acked_version())
            .is_err()
        {
            // Mismatch is an in-flight crossing, not an attack: the channel
            // is ratchet-authenticated and the state machine rejects
            // explicitly (DC-02) — nothing to record, nothing to error.
        }
        let reply = match ack.broadcast_json() {
            Some(echo) => {
                let held_version = persist_carried_broadcast(storage, sender_id, contact, echo)?;
                tracker.record_peer_registry(held_version);
                Some(RegistryReplyNeeded {
                    sender_id: sender_id.to_string(),
                    acked_version: held_version,
                    push_nonce: *ack.push_nonce(),
                })
            }
            None => None,
        };
        storage
            .registry_activation()
            .save_activation(sender_id, &tracker)?;
        storage.ratchets().save_ratchet_state_for_device(
            sender_id,
            peer_device_id,
            ratchet,
            is_initiator,
        )?;
        Ok(reply)
    })();
    match txn {
        Ok(reply) => {
            storage.commit()?;
            Ok(ReceiveOutcome::RegistryAckReceived {
                sender_id: sender_id.to_string(),
                reply,
            })
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

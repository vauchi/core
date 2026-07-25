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

/// Handle a genesis-sealed RegistryPush (F4 slice 4b).
///
/// The envelope's header broadcast is the authority — identity-signed by
/// the sender and verified at persist. Unlike an alert genesis, no `[0;32]`
/// session is persisted (a handshake genesis is routing bootstrap, not an
/// alert cold start) and no replay nonce is burned: processing is
/// idempotent and the genesis rate limiter bounds volume upstream.
pub(crate) fn receive_genesis_registry_push(
    identity: &crate::identity::Identity,
    storage: &Storage,
    sender_id: &str,
    contact: &Contact,
    header_broadcast_json: &[u8],
    push: &RegistryPushPayload,
) -> Result<ReceiveOutcome, CardUpdateError> {
    storage.begin_transaction()?;
    let txn = (|| -> Result<u64, CardUpdateError> {
        let header_version =
            persist_carried_broadcast(storage, sender_id, contact, header_broadcast_json)?;
        let inner_version =
            persist_carried_broadcast(storage, sender_id, contact, push.broadcast_json())?;
        let held = header_version.max(inner_version);
        let mut tracker = load_tracker(storage, sender_id)?;
        tracker.record_peer_registry(held);
        storage
            .registry_activation()
            .save_activation(sender_id, &tracker)?;
        Ok(held)
    })();
    match txn {
        Ok(held) => {
            storage.commit()?;
            journal_handshake_state_for_siblings(identity, storage, sender_id);
            Ok(ReceiveOutcome::RegistryPushReceived(RegistryReplyNeeded {
                sender_id: sender_id.to_string(),
                acked_version: held,
                push_nonce: *push.push_nonce(),
            }))
        }
        Err(e) => {
            storage.rollback();
            Err(e)
        }
    }
}

/// Handle a genesis-sealed RegistryAck (F4 slice 4b).
///
/// The header broadcast always carries the sender's registry, so even an
/// echo-less genesis ack delivers what the receiver needs for routing. A
/// confirming reply fires only when THIS ack transitioned us to `Active`
/// — that bound terminates the handshake (an already-Active receiver
/// processes idempotently and stays silent).
pub(crate) fn receive_genesis_registry_ack(
    identity: &crate::identity::Identity,
    storage: &Storage,
    sender_id: &str,
    contact: &Contact,
    header_broadcast_json: &[u8],
    ack: &RegistryAckPayload,
) -> Result<ReceiveOutcome, CardUpdateError> {
    use crate::sync::registry_activation::ActivationState;

    storage.begin_transaction()?;
    let txn = (|| -> Result<Option<RegistryReplyNeeded>, CardUpdateError> {
        let mut tracker = load_tracker(storage, sender_id)?;
        let was_active = tracker.state() == ActivationState::Active;
        let header_version =
            persist_carried_broadcast(storage, sender_id, contact, header_broadcast_json)?;
        let mut held = header_version;
        if let Some(echo) = ack.broadcast_json() {
            held = held.max(persist_carried_broadcast(
                storage, sender_id, contact, echo,
            )?);
        }
        tracker.record_peer_registry(held);
        if tracker
            .record_ack(ack.push_nonce(), ack.acked_version())
            .is_err()
        {
            // In-flight crossing — tolerated without state change (DC-02).
        }
        let activated_now = !was_active && tracker.state() == ActivationState::Active;
        storage
            .registry_activation()
            .save_activation(sender_id, &tracker)?;
        Ok(activated_now.then(|| RegistryReplyNeeded {
            sender_id: sender_id.to_string(),
            acked_version: held,
            push_nonce: *ack.push_nonce(),
        }))
    })();
    match txn {
        Ok(reply) => {
            storage.commit()?;
            journal_handshake_state_for_siblings(identity, storage, sender_id);
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

/// Repair a diverged device-scoped session: drop the corrupt row and
/// demote the activation so the scanner re-runs the handshake (F4 plan
/// trigger 4, Kimi corrupt-session condition).
///
/// Best-effort — the caller is already returning a deterministic decrypt
/// failure for the blob; a repair that itself fails will simply retry on
/// the next divergent receive.
pub(crate) fn repair_device_session(
    identity: &crate::identity::Identity,
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
) {
    let attempt = (|| -> Result<(), crate::storage::StorageError> {
        storage
            .ratchets()
            .delete_ratchet_state_for_device(sender_id, peer_device_id)?;
        let mut tracker = storage
            .registry_activation()
            .load_activation(sender_id)?
            .unwrap_or_default();
        tracker.record_session_repair();
        storage
            .registry_activation()
            .save_activation(sender_id, &tracker)
    })();
    match attempt {
        Ok(()) => journal_handshake_state_for_siblings(identity, storage, sender_id),
        Err(error) => tracing::warn!("device session repair failed: {error}"),
    }
}

/// Journal the contact's held registry and activation state for this
/// identity's linked devices. No-op for single-device identities.
///
/// Best-effort AFTER the state commit (`record_local_change` opens its own
/// transaction, so it cannot nest inside the receive transaction — same
/// seam as `ContactCardUpdated` fan-out). Loss scope (review F3): a lost
/// journal of UNCONFIRMED state re-journals on the scanner's next push;
/// a lost journal of the Active transition is repaired only by later
/// handshake activity (registry change, repair re-push) or by the
/// device-link full sync, which snapshots activation state — an existing
/// sibling that misses the transition keeps working via owner-sync
/// mediation until one of those occurs.
pub(crate) fn journal_handshake_state_for_siblings(
    identity: &crate::identity::Identity,
    storage: &Storage,
    contact_id: &str,
) {
    if let Err(error) = try_journal_for_siblings(identity, storage, contact_id) {
        tracing::warn!("sibling handshake journal failed: {error}");
    }
}

fn try_journal_for_siblings(
    identity: &crate::identity::Identity,
    storage: &Storage,
    contact_id: &str,
) -> Result<(), String> {
    use crate::sync::device_sync::SyncItem;

    let Some(own_registry) = storage
        .device()
        .load_device_registry()
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if own_registry.device_count() <= 1 {
        return Ok(());
    }
    let now = storage.clock().unix_seconds();
    let mut orchestrator = super::device_orchestrator::DeviceSyncOrchestrator::load(
        storage,
        identity.create_device_info(now),
        own_registry,
    )
    .map_err(|e| e.to_string())?;
    if let Some(broadcast) = storage
        .device()
        .load_contact_device_registry(contact_id)
        .map_err(|e| e.to_string())?
    {
        orchestrator
            .record_local_change(SyncItem::ContactRegistryReceived {
                contact_id: contact_id.to_string(),
                registry_json: broadcast.to_json(),
                version: broadcast.version(),
                timestamp: now,
            })
            .map_err(|e| e.to_string())?;
    }
    if let Some(tracker) = storage
        .registry_activation()
        .load_activation(contact_id)
        .map_err(|e| e.to_string())?
    {
        let (push_nonce, pushed_version) = match tracker.outstanding_push() {
            Some((nonce, version)) => (Some(nonce.to_vec()), Some(version)),
            None => (None, None),
        };
        orchestrator
            .record_local_change(SyncItem::ContactActivationChanged {
                contact_id: contact_id.to_string(),
                push_nonce,
                pushed_version,
                our_version_acked: tracker.our_version_acked(),
                peer_version_held: tracker.peer_version_held(),
                timestamp: now,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Handle a received RegistryPush inside the receive transaction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn receive_registry_push(
    identity: &crate::identity::Identity,
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
            journal_handshake_state_for_siblings(identity, storage, sender_id);
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
    identity: &crate::identity::Identity,
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
        use crate::sync::registry_activation::ActivationState;

        let mut tracker = load_tracker(storage, sender_id)?;
        let was_active = tracker.state() == ActivationState::Active;
        if tracker
            .record_ack(ack.push_nonce(), ack.acked_version())
            .is_err()
        {
            // Mismatch is an in-flight crossing, not an attack: the channel
            // is ratchet-authenticated and the state machine rejects
            // explicitly (DC-02) — nothing to record, nothing to error.
        }
        let mut held_version = None;
        if let Some(echo) = ack.broadcast_json() {
            let version = persist_carried_broadcast(storage, sender_id, contact, echo)?;
            tracker.record_peer_registry(version);
            held_version = Some(version);
        }
        // Reply only on OUR not-Active -> Active transition (mirrors the
        // genesis handler): an already-Active tracker processing a replayed
        // echo-ack must stay silent, or a malicious authenticated contact
        // could force an unbounded reply stream against the never-cleared
        // outstanding push (review finding F1). The peer's un-confirmed
        // registry still converges via its scanner re-push.
        let activated_now = !was_active && tracker.state() == ActivationState::Active;
        let reply = match (activated_now, held_version) {
            (true, Some(version)) => Some(RegistryReplyNeeded {
                sender_id: sender_id.to_string(),
                acked_version: version,
                push_nonce: *ack.push_nonce(),
            }),
            _ => None,
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
            journal_handshake_state_for_siblings(identity, storage, sender_id);
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

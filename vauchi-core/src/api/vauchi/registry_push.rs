// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 handshake outbound side — queuing ack replies (ADR-064 Amendment
//! 2026-07-25).
//!
//! An ack echoes our own identity-signed registry exactly when no push of
//! ours is already in flight: the echo *is* our push (riding the peer's
//! handshake nonce), while an outstanding push means our half is already
//! progressing on its own track and a crossing handshake must not clobber
//! it. Ack blobs ride the ordinary pending queue as `card_delta`, so they
//! are indistinguishable from card updates on the wire (ADR-032 posture,
//! same as reciprocity confirmations and safety alerts).

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::sync::RegistryReplyNeeded;
use crate::rng::SecureRngExt;
use crate::storage::{PendingUpdate, UpdateStatus};
use crate::sync::delta::VersionedPayload;
use crate::sync::registry_activation::RegistryAckPayload;

impl Vauchi {
    /// Queue the ack a received registry push (or echo) asked for.
    ///
    /// Failure tolerance is the caller's: a lost or failed ack only delays
    /// the peer's activation until their next re-push.
    pub fn queue_registry_ack(&self, reply: &RegistryReplyNeeded) -> VauchiResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let Some(contact) = self.storage.contacts().load_contact(&reply.sender_id)? else {
            return Ok(());
        };
        // ADR-056: a blocked contact receives nothing, silently.
        if contact.is_blocked() {
            return Ok(());
        }

        let mut tracker = self
            .storage
            .registry_activation()
            .load_activation(&reply.sender_id)?
            .unwrap_or_default();
        let echo = if tracker.outstanding_push().is_none() {
            let broadcast = self.own_registry_broadcast(identity)?;
            tracker.record_push_sent(reply.push_nonce, broadcast.version());
            Some(broadcast.to_json().into_bytes())
        } else {
            None
        };

        let ack = RegistryAckPayload::new(reply.push_nonce, reply.acked_version, echo)
            .map_err(|error| VauchiError::Serialization(error.to_string()))?;
        let payload = VersionedPayload::encode_registry_ack(&ack);
        let prepared = self.encrypt_payload_for_contact_devices(identity, &contact, &payload)?;

        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            for item in prepared {
                if item.persist_session {
                    self.storage.ratchets().save_ratchet_state_for_device(
                        &reply.sender_id,
                        &item.peer_device_id,
                        &item.session,
                        item.is_initiator,
                    )?;
                }
                let update = PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: reply.sender_id.clone(),
                    update_type: "card_delta".to_string(),
                    payload: item.encrypted,
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: None,
                };
                self.storage.pending().queue_update(&update)?;
            }
            self.storage
                .registry_activation()
                .save_activation(&reply.sender_id, &tracker)?;
            Ok(())
        })
    }
}

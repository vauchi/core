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
use crate::sync::registry_activation::{ActivationState, RegistryAckPayload, RegistryPushPayload};

impl Vauchi {
    /// Seal a payload into the stateless genesis envelope for a contact
    /// with no session and no registry (alerts and F4 handshake payloads).
    /// The initiator session is deliberately never persisted (ADR-064
    /// Amendment 2026-07-24, guarded invariant 2).
    pub(super) fn genesis_seal_for_cold_start(
        &self,
        identity: &crate::identity::Identity,
        ex: &crate::contact::ExchangedData,
        payload: &[u8],
    ) -> VauchiResult<super::propagation::PreparedDevicePayload> {
        let broadcast = self.own_registry_broadcast(identity)?;
        let epoch = crate::network::mailbox_token::current_day_epoch(self.clock.unix_seconds());
        let (message, session) = crate::exchange::genesis::GenesisEnvelope::seal(
            &ex.shared_key,
            identity,
            &ex.public_key,
            &broadcast,
            epoch,
            payload,
        )
        .map_err(|error| VauchiError::Crypto(format!("genesis seal: {error}")))?;
        let encrypted = serde_json::to_vec(&message)
            .map_err(|error| VauchiError::Serialization(error.to_string()))?;
        Ok(super::propagation::PreparedDevicePayload {
            peer_device_id: [0; 32],
            encrypted,
            session,
            is_initiator: true,
            persist_session: false,
        })
    }

    /// Queue a RegistryPush to every exchanged contact that has not
    /// confirmed our current registry version (the F4 vouched push).
    ///
    /// Level-triggered: device-link completion, exchange completion, and
    /// registry changes all leave `own version != acked version`, and this
    /// sync-cycle scanner reconciles that state — so a crash between any
    /// trigger and its push self-heals on the next tick. Returns how many
    /// pushes were queued.
    pub fn queue_registry_pushes(&self) -> VauchiResult<usize> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let own_broadcast = self.own_registry_broadcast(identity)?;
        let own_version = own_broadcast.version();
        let broadcast_bytes = own_broadcast.to_json().into_bytes();
        let now = self.clock.unix_seconds();
        let mut queued = 0;

        for contact in self.storage.contacts().list_contacts()? {
            // ADR-056: a blocked contact receives nothing, silently.
            if contact.is_blocked() || contact.kind().exchanged_data().is_none() {
                continue;
            }
            let contact_id = contact.id().to_string();
            let mut tracker = self
                .storage
                .registry_activation()
                .load_activation(&contact_id)?
                .unwrap_or_default();
            let confirmed = tracker.state() == ActivationState::Active
                && tracker.our_version_acked() == Some(own_version);
            if confirmed {
                continue;
            }
            // One handshake message in flight per contact — like reciprocity
            // confirmations, a queued undelivered push must not pile up.
            if self.storage.pending().count_pending_updates(&contact_id)? > 0 {
                continue;
            }

            let mut nonce = [0u8; 32];
            self.rng.fill_bytes(&mut nonce);
            let push = RegistryPushPayload::new(nonce, broadcast_bytes.clone())
                .map_err(|error| VauchiError::Serialization(error.to_string()))?;
            let payload = VersionedPayload::encode_registry_push(&push);
            // Pre-activation handshake messages ALWAYS ride the stateless
            // genesis envelope: a session-less orphaned sibling opens it
            // statelessly, and a live sessioned peer opens it via the
            // decrypt-failure genesis fallback — one wire form reaches every
            // possible receiver state, and the [0;32] path can never strand
            // the handshake behind a dead exchanging device.
            let Some(ex) = contact.kind().exchanged_data() else {
                continue;
            };
            let prepared = vec![self.genesis_seal_for_cold_start(identity, ex, &payload)?];
            tracker.record_push_sent(nonce, own_version);
            self.storage.with_savepoint(|| -> VauchiResult<()> {
                for item in prepared {
                    if item.persist_session {
                        self.storage.ratchets().save_ratchet_state_for_device(
                            &contact_id,
                            &item.peer_device_id,
                            &item.session,
                            item.is_initiator,
                        )?;
                    }
                    let target_device_id = item.target_device_id();
                    let update = PendingUpdate {
                        id: self.rng.uuid_v4(),
                        contact_id: contact_id.clone(),
                        update_type: "card_delta".to_string(),
                        payload: item.encrypted,
                        created_at: now,
                        retry_count: 0,
                        status: UpdateStatus::Pending,
                        target_relay_url: None,
                        target_device_id,
                    };
                    self.storage.pending().queue_update(&update)?;
                }
                self.storage
                    .registry_activation()
                    .save_activation(&contact_id, &tracker)?;
                Ok(())
            })?;
            crate::api::sync::journal_handshake_state_for_siblings(
                identity,
                &self.storage,
                &contact_id,
            );
            queued += 1;
        }
        Ok(queued)
    }

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
        // Pre-activation acks ride the stateless genesis envelope like
        // pushes: the peer may be a session-less orphaned sibling that can
        // only open genesis, while a live sessioned peer opens it via the
        // decrypt-failure fallback. Once Active, the ordinary path applies
        // (per-device, resolvable — Active implies the peer holds our
        // registry) — EXCEPT when a fresh device-pair session puts us on
        // the responder side, which cannot send first: without the genesis
        // fallback the handshake livelocks, because our scanner skips
        // Active contacts and the peer only activates through this very
        // ack (caught by the ordering-flipped un-orphaning test).
        let genesis_sealed = |this: &Self| -> VauchiResult<_> {
            let ex = contact
                .kind()
                .exchanged_data()
                .ok_or_else(|| VauchiError::InvalidState("contact not exchanged".into()))?;
            Ok(vec![
                this.genesis_seal_for_cold_start(identity, ex, &payload)?,
            ])
        };
        let prepared = if tracker.state() == ActivationState::Active {
            match self.encrypt_payload_for_contact_devices(identity, &contact, &payload) {
                Ok(prepared) => prepared,
                Err(VauchiError::InvalidState(_)) | Err(VauchiError::NotFound(_)) => {
                    genesis_sealed(self)?
                }
                Err(error) => return Err(error),
            }
        } else {
            genesis_sealed(self)?
        };

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
                let target_device_id = item.target_device_id();
                let update = PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: reply.sender_id.clone(),
                    update_type: "card_delta".to_string(),
                    payload: item.encrypted,
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: None,
                    target_device_id,
                };
                self.storage.pending().queue_update(&update)?;
            }
            self.storage
                .registry_activation()
                .save_activation(&reply.sender_id, &tracker)?;
            Ok(())
        })?;
        crate::api::sync::journal_handshake_state_for_siblings(
            identity,
            &self.storage,
            &reply.sender_id,
        );
        Ok(())
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reciprocity confirmation send driver (design P3 Slice B).
//!
//! On each sync cycle, for every still-`Pending` confirmable contact, queue a
//! signed [`ReciprocityConfirmPayload`] carrying *our* confirmation token so the
//! peer can resolve reciprocity to `Confirmed` after the parties have parted
//! (the relay-sync tier). The payload rides the same ratchet-encrypted
//! card-update envelope as everything else — indistinguishable on the wire.
//!
//! The token is *derived on demand* from the contact's stored `shared_key`
//! (`Contact::derive_reciprocity_tokens`) — no stored token, no migration. A
//! contact with no shared key (imported) yields no token and is skipped, so the
//! confirmable-set and the Pending gate together form the syncability gate.
//!
//! A peer whose confirmation reaches us while it still waits for ours gets an
//! answer (private#663): our own confirmation, marked as one, so it is never
//! answered in turn.
//!
//! [`ReciprocityConfirmPayload`]: crate::sync::delta::ReciprocityConfirmPayload

use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    /// Queue a reciprocity confirmation for every still-`Pending` confirmable
    /// contact. Returns the number queued. Idempotent per cycle and
    /// self-terminating: run *after* the receive phase (which may have just
    /// flipped some contacts to `Confirmed`), so a mutually-confirmed pair stops
    /// re-sending — the exchange converges. Contacts that are not `Pending`, are
    /// imported (no derivable token), blocked, or lack a ratchet are skipped.
    ///
    /// Driven by the sync cycle (`sync_inner`, before the send phase). Also
    /// `pub` so integration tests can exercise the send path without a relay.
    pub fn queue_reciprocity_confirmations(&self) -> VauchiResult<usize> {
        use crate::exchange::reciprocity::Reciprocity;

        let now = self.clock.unix_seconds();
        let mut queued = 0;
        for contact in &self.storage.contacts().list_contacts()? {
            if contact.reciprocity(now) == Reciprocity::Pending
                && self.queue_reciprocity_payload(contact, true)?
            {
                queued += 1;
            }
        }
        Ok(queued)
    }

    /// Queue our confirmation, as an answer, for a peer still waiting for it
    /// (private#663). Returns the number queued. Driven by the receive phase.
    pub fn queue_reciprocity_answer(&self, contact_id: &str) -> VauchiResult<usize> {
        let Some(contact) = self.storage.contacts().load_contact(contact_id)? else {
            return Ok(0);
        };
        Ok(usize::from(
            self.queue_reciprocity_payload(&contact, false)?,
        ))
    }

    /// Queue our signed confirmation token for `contact`; `sender_waiting`
    /// says whether we still wait for theirs. Returns whether one was queued.
    fn queue_reciprocity_payload(
        &self,
        contact: &crate::contact::Contact,
        sender_waiting: bool,
    ) -> VauchiResult<bool> {
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{ReciprocityConfirmPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        if contact.is_blocked() {
            return Ok(false);
        }
        let Some((our_token, _expected)) =
            contact.derive_reciprocity_tokens(identity.signing_public_key())
        else {
            return Ok(false);
        };
        let Some(recipient_pk) = contact.public_key().copied() else {
            return Ok(false);
        };
        let contact_id = contact.id().to_string();
        // Slice D: one confirmation in flight per contact is enough — it is
        // idempotent and ratchet-deduped on receipt. Skip while any prior
        // update is still queued, so an unreachable relay (or an offline
        // peer whose mailbox we can't yet reach) can't pile up duplicate
        // confirmations cycle after cycle. Once the queued one delivers and
        // clears, the next Pending cycle re-queues.
        if self.storage.pending().count_pending_updates(&contact_id)? > 0 {
            return Ok(false);
        }
        let confirm = if sender_waiting {
            ReciprocityConfirmPayload::new(*our_token, identity, &recipient_pk)
        } else {
            ReciprocityConfirmPayload::answer(*our_token, identity, &recipient_pk)
        };
        let payload = VersionedPayload::encode_reciprocity(&confirm);
        let prepared = match self.encrypt_payload_for_contact_devices(identity, contact, &payload) {
            Ok(prepared) => prepared,
            Err(VauchiError::NotFound(_))
            | Err(VauchiError::InvalidState(_))
            | Err(VauchiError::SendDeferred) => return Ok(false),
            Err(error) => return Err(error),
        };
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            for payload in prepared {
                if payload.persist_session {
                    self.storage.ratchets().save_ratchet_state_for_device(
                        &contact_id,
                        &payload.peer_device_id,
                        &payload.session,
                        payload.is_initiator,
                    )?;
                }
                let target_device_id = payload.target_device_id();
                let update = PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: contact_id.clone(),
                    // Indistinguishable from a card update on the wire (ADR-032),
                    // like the safety-alert send path.
                    update_type: "card_delta".to_string(),
                    payload: payload.encrypted,
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: None,
                    target_device_id,
                };
                self.storage.pending().queue_update(&update)?;
            }
            Ok(())
        })?;
        Ok(true)
    }
}

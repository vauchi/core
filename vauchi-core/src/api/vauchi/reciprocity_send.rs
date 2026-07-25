// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{ReciprocityConfirmPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let our_id = *identity.signing_public_key();
        let now = self.clock.unix_seconds();
        let contacts = self.storage.contacts().list_contacts()?;
        let mut queued = 0;

        for contact in &contacts {
            // Gate: only confirmable exchanges still awaiting confirmation.
            if contact.reciprocity(now) != Reciprocity::Pending || contact.is_blocked() {
                continue;
            }
            let Some((our_token, _expected)) = contact.derive_reciprocity_tokens(&our_id) else {
                continue;
            };
            let Some(recipient_pk) = contact.public_key().copied() else {
                continue;
            };
            let contact_id = contact.id().to_string();
            // Slice D: one confirmation in flight per contact is enough — it is
            // idempotent and ratchet-deduped on receipt. Skip while any prior
            // update is still queued, so an unreachable relay (or an offline
            // peer whose mailbox we can't yet reach) can't pile up duplicate
            // confirmations cycle after cycle. Once the queued one delivers and
            // clears, the next Pending cycle re-queues.
            if self.storage.pending().count_pending_updates(&contact_id)? > 0 {
                continue;
            }
            let confirm = ReciprocityConfirmPayload::new(*our_token, identity, &recipient_pk);
            let payload = VersionedPayload::encode_reciprocity(&confirm);
            let prepared =
                match self.encrypt_payload_for_contact_devices(identity, contact, &payload) {
                    Ok(prepared) => prepared,
                    Err(VauchiError::NotFound(_)) | Err(VauchiError::InvalidState(_)) => continue,
                    Err(error) => return Err(error),
                };
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
            queued += 1;
        }

        Ok(queued)
    }
}

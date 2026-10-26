// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Cover for a duress session's own-card edit (ADR-032, vauchi/private#811).
//!
//! A real edit marks the card dirty and the next sync tick queues one
//! `card_delta` per contact device. A duress edit changes only the duress
//! copy, but a coercer watching the pending count or the wire must see the
//! same thing — so the tick seals, for every contact the real path would
//! reach, a cover payload of the length that contact's delta would have,
//! through the same ratchet sessions. The contact's receiver opens it as
//! nothing (`ReceiveOutcome::Cover`). The marker lives in the session, as
//! every other trace of a duress session does.

use crate::reminders::OwnCardRepropagateState;
use crate::rng::SecureRngExt;
use crate::storage::{PendingUpdate, UpdateStatus};
use crate::sync::delta::encode_cover_payload;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    /// The duress counterpart of `mark_own_card_repropagate`: a fresh edit
    /// owes a cover pass and resets its retry budget.
    pub(super) fn mark_owed_cover(&self) {
        self.concealed_duress().owed_cover = OwnCardRepropagateState {
            needs_repropagate: true,
            failed_attempts: 0,
        };
    }

    /// The duress counterpart of `run_owed_repropagation`, with the same
    /// retry budget so a failing contact backs off the same way.
    pub(super) fn run_owed_cover(&self) -> VauchiResult<()> {
        let mut state = self.concealed_duress().owed_cover.clone();
        if !state.should_run() {
            return Ok(());
        }

        let mut all_ok = true;
        for contact in self.storage.contacts().list_contacts()? {
            if self.cover_or_defer(contact.id()).is_err() {
                all_ok = false;
            }
        }

        self.concealed_duress().owed_cover = if all_ok {
            OwnCardRepropagateState::default()
        } else {
            state.failed_attempts = state.failed_attempts.saturating_add(1);
            state
        };
        Ok(())
    }

    fn cover_or_defer(&self, contact_id: &str) -> VauchiResult<()> {
        match self.queue_cover_for_contact(contact_id) {
            Err(VauchiError::SendDeferred) => Ok(()),
            other => other,
        }
    }

    /// Queues for `contact_id` what `repropagate_to_contact` would queue
    /// for the duress copy, with cover in place of the delta: the same
    /// recipients, the same length, the same ratchet steps. Nothing about
    /// the real card — CEK, delta version, last-sent baselines — moves.
    fn queue_cover_for_contact(&self, contact_id: &str) -> VauchiResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let duress_card = self
            .storage
            .contacts()
            .duress_own_card()
            .load()?
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;

        let Some(owed) = self.owed_card_delta(&duress_card, &contact)? else {
            return Ok(());
        };
        if owed.delta.is_empty() {
            return Ok(());
        }

        let delta_len = self
            .cek_wrapped_card_delta(identity, &contact, owed.delta)?
            .payload_bytes
            .len();
        let mut filler = vec![0u8; delta_len.saturating_sub(1)];
        self.rng.fill_bytes(&mut filler);
        let cover = encode_cover_payload(&filler);

        let prepared = self.encrypt_payload_for_contact_devices(identity, &contact, &cover)?;
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            for payload in &prepared {
                if payload.persist_session {
                    self.storage.ratchets().save_ratchet_state_for_device(
                        contact_id,
                        &payload.peer_device_id,
                        &payload.session,
                        payload.is_initiator,
                    )?;
                }
                self.storage.pending().queue_update(&PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: contact_id.to_string(),
                    update_type: "card_delta".to_string(),
                    payload: payload.encrypted.clone(),
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: None,
                    target_device_id: payload.target_device_id(),
                })?;
            }
            Ok(())
        })
    }
}

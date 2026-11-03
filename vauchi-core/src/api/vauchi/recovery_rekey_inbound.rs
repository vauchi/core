// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact taking in and accepting a rekey offer (vauchi/private#796,
//! steps 5a and 5b, D18).
//!
//! The recovered contact asks for the rekey; this device is not asked to
//! confirm. It accepts an offer when it is provably the recovered
//! contact's (`RecoveryProof::proves_the_recovery_to`): the new identity
//! signed it, the proof opens for one of this device's contacts, names
//! that same identity, and carries enough guardian tokens the old key
//! signed or enough vouchers from this device's own contacts. Anything
//! else is dropped without a trace, and every offer in a duress session.
//! Accepting answers it (`recovery_acceptance.spthy`, John_answers) and
//! moves the contact to the new key's id with a fresh ratchet.

use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::exchange::X3DHKeyPair;
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::recovery::RecoveryProof;
use crate::recovery::rekey::{RekeyAnswer, RekeyOffer, rekey_shared_key};

use super::Vauchi;
use super::recovery_rekey::{REKEY_RESEND_WINDOW_SECS, RekeyDelivery};

/// An answer not yet followed by the new relationship goes again after a
/// day; its exchange key stays the one the contact already switched to.
const REKEY_ANSWER_RESEND_SECS: u64 = 86_400;

/// What [`Vauchi::take_rekey_offer`] made of an offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RekeyOfferTaken {
    /// Accepted: the recovered contact now has this id, its new key's.
    Accepted { contact_id: String },
    /// Not an offer this device can act on; nothing changed.
    Dropped,
}

impl Vauchi {
    /// Takes in a rekey offer from the relay and accepts it when it is
    /// provably the recovered contact's; drops it otherwise.
    ///
    /// # Errors
    /// Storage errors only, which change nothing; a refused offer is
    /// [`RekeyOfferTaken::Dropped`].
    pub fn take_rekey_offer(&self, blob: &[u8]) -> VauchiResult<RekeyOfferTaken> {
        let Some(identity) = self.identity.as_ref() else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        if self.in_duress_mode() {
            return Ok(RekeyOfferTaken::Dropped);
        }
        let Ok(offer) = RekeyOffer::decode(blob) else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        let Some(contact_id) = self.proven_offer(&offer)? else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        let new_pk = *offer.new_pk();
        let new_id = hex::encode(new_pk);
        // A contact with the new key already (met in person again) is
        // folded into, not moved onto (D21); until then the offer waits.
        if self.storage.contacts().load_contact(&new_id)?.is_some() {
            return Ok(RekeyOfferTaken::Dropped);
        }
        let contact = self
            .storage
            .contacts()
            .load_contact(&contact_id)?
            .ok_or_else(|| VauchiError::NotFound(contact_id.clone()))?;
        let old_shared = contact
            .shared_key()
            .ok_or_else(|| VauchiError::InvalidState("The contact has no shared key".into()))?
            .clone();
        let exchange = X3DHKeyPair::generate();
        let Ok(shared) = rekey_shared_key(
            &exchange,
            offer.exchange_key(),
            &new_pk,
            identity.signing_public_key(),
        ) else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        let answer = RekeyAnswer::create(identity, *exchange.public_key(), *offer.exchange_key());
        let ratchet = DoubleRatchetState::initialize_responder(&shared, exchange);
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage.move_contact_id(&contact_id, &new_id)?;
            let mut moved = self
                .storage
                .contacts()
                .load_contact(&new_id)?
                .ok_or_else(|| VauchiError::NotFound(new_id.clone()))?;
            moved
                .accept_recovery(new_pk, shared.clone(), now)
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            self.storage.contacts().save_contact(&moved)?;
            let ratchets = self.storage.ratchets();
            ratchets.delete_ratchet_states_for_contact(&new_id)?;
            ratchets.save_ratchet_state(&new_id, &ratchet, false)?;
            self.storage
                .recovery()
                .keep_rekey_answer(&new_id, &answer.encode(), &old_shared)?;
            Ok(())
        })?;
        Ok(RekeyOfferTaken::Accepted { contact_id: new_id })
    }

    /// The rekey answers due now, each to `token(old shared key, new key,
    /// day)`, the mailbox the recovered contact's new phone polls. One that
    /// went out in the last day waits; after 14 days they stop (D18).
    ///
    /// # Errors
    /// Storage errors.
    pub fn rekey_answer_deliveries(&self) -> VauchiResult<Vec<RekeyDelivery>> {
        let now = self.clock.unix_seconds();
        let day = current_day_epoch(now);
        let mut deliveries = Vec::new();
        for (contact_id, blob, old_shared, kept_at, sent_at) in
            self.storage.recovery().kept_rekey_answers()?
        {
            if now >= kept_at.saturating_add(REKEY_RESEND_WINDOW_SECS) {
                continue;
            }
            if sent_at.is_some_and(|at| now < at.saturating_add(REKEY_ANSWER_RESEND_SECS)) {
                continue;
            }
            let Some(contact) = self.storage.contacts().load_contact(&contact_id)? else {
                continue;
            };
            let Some(new_pk) = contact.public_key() else {
                continue;
            };
            deliveries.push(RekeyDelivery {
                token_hex: token_hex(&compute_mailbox_token(old_shared.as_bytes(), new_pk, day)),
                contact_id,
                blob,
            });
        }
        Ok(deliveries)
    }

    /// Records that the answer to `contact_id`'s rekey went out now.
    ///
    /// # Errors
    /// Storage errors.
    pub fn record_rekey_answer_sent(&self, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .recovery()
            .record_rekey_answer_sent(contact_id, self.clock.unix_seconds())?)
    }

    /// Sends the rekey answers due now, recording each one that went out.
    /// Returns how many went out.
    ///
    /// # Errors
    /// Storage errors while preparing them.
    pub fn send_rekey_answers(&self) -> VauchiResult<usize> {
        use base64::Engine;
        let deliveries = self.rekey_answer_deliveries()?;
        if deliveries.is_empty() {
            return Ok(0);
        }
        let transport = self.create_guardian_transport();
        let mut sent = 0;
        for delivery in &deliveries {
            let blob_b64 = base64::engine::general_purpose::STANDARD.encode(&delivery.blob);
            if transport
                .send_update(&delivery.token_hex, &blob_b64, None)
                .is_ok()
            {
                self.record_rekey_answer_sent(&delivery.contact_id)?;
                sent += 1;
            }
        }
        Ok(sent)
    }

    /// The recovered contact's id when `offer` is signed by the identity it
    /// names, its proof opens for an exchanged, unblocked contact here,
    /// names that same identity, and proves the recovery to this device.
    fn proven_offer(&self, offer: &RekeyOffer) -> VauchiResult<Option<String>> {
        if !offer.verify() {
            return Ok(None);
        }
        let contacts = self.storage.contacts().list_contacts()?;
        let opened = if RecoveryProof::is_sealed(offer.proof()) {
            RecoveryProof::open_sealed_for_contacts(offer.proof(), &contacts)
        } else {
            RecoveryProof::from_bytes(offer.proof())
        };
        let Ok(proof) = opened else {
            return Ok(None);
        };
        if proof.new_pk().as_bytes() != offer.new_pk() {
            return Ok(None);
        }
        let settings = self
            .storage
            .recovery()
            .load_recovery_settings()?
            .unwrap_or_default();
        if !proof.proves_the_recovery_to(&contacts, &settings) {
            return Ok(None);
        }
        let contact_id = hex::encode(proof.old_pk());
        let Some(contact) = contacts.iter().find(|contact| contact.id() == contact_id) else {
            return Ok(None);
        };
        if contact.is_blocked() || contact.public_key().is_none() {
            return Ok(None);
        }
        Ok(Some(contact_id))
    }
}

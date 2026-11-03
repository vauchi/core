// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact taking in and accepting a rekey offer (vauchi/private#796,
//! steps 5a and 5b, D18).
//!
//! The recovered contact asks for the rekey; this device is not asked to
//! confirm. It accepts an offer only with a [`RecoveryAuthorization`] for
//! it, which only a policy-guardian decision certificate will grant
//! (design 2026-11-03-pinned-guardian-policy, P15, P18). None exists yet,
//! so every offer is dropped: vouchers, guardian tokens and old-key
//! signatures authorize nothing, since two legitimate attempts could each
//! gather vouchers and a thief with the old device signs the rest. Every
//! offer is dropped in a duress session too. Accepting answers it
//! (`recovery_acceptance.spthy`, John_answers) and moves the contact to
//! the new key's id with a fresh ratchet.

use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::exchange::X3DHKeyPair;
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::recovery::authorization::RecoveryAuthorization;
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
        let Ok(offer) = RekeyOffer::decode(blob) else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        match authorize_recovery(&offer) {
            Some(authorization) => self.take_rekey_offer_authorized(blob, &authorization),
            None => Ok(RekeyOfferTaken::Dropped),
        }
    }

    /// Takes in a rekey offer `authorization` permits: signed by the new
    /// identity it names, for an exchanged, unblocked contact holding the
    /// authorization's old key. Drops it otherwise.
    ///
    /// # Errors
    /// Storage errors only, which change nothing.
    pub fn take_rekey_offer_authorized(
        &self,
        blob: &[u8],
        authorization: &RecoveryAuthorization,
    ) -> VauchiResult<RekeyOfferTaken> {
        let Some(identity) = self.identity.as_ref() else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        if self.in_duress_mode() {
            return Ok(RekeyOfferTaken::Dropped);
        }
        let Ok(offer) = RekeyOffer::decode(blob) else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        if !offer.verify() || offer.new_pk() != authorization.new_pk() {
            return Ok(RekeyOfferTaken::Dropped);
        }
        let contact_id = hex::encode(authorization.old_pk());
        match self.storage.contacts().load_contact(&contact_id)? {
            Some(contact) if !contact.is_blocked() && contact.public_key().is_some() => {}
            _ => return Ok(RekeyOfferTaken::Dropped),
        }
        let new_pk = *offer.new_pk();
        let new_id = hex::encode(new_pk);
        // Met again in person, the contact already has the new key and the
        // meeting's relationship: the old one folds into it and nothing is
        // answered (D21).
        if self.storage.contacts().load_contact(&new_id)?.is_some() {
            self.storage.fold_contact_into(&contact_id, &new_id)?;
            self.record_contact_rekeyed(&contact_id, &new_id)?;
            return Ok(RekeyOfferTaken::Accepted { contact_id: new_id });
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
        self.record_contact_rekeyed(&contact_id, &new_id)?;
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
            // Kept before its time was recorded (migration v108): its 14
            // days start now.
            let kept_at = if kept_at == 0 {
                self.storage
                    .recovery()
                    .record_rekey_answer_kept(&contact_id, now)?;
                now
            } else {
                kept_at
            };
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
}

/// The authorization for `offer`. None is granted until the policy
/// guardians' decision certificate exists (P15, P18): a confidence score
/// from vouchers cannot keep two legitimate attempts from both being
/// accepted (P11), and the old key's signatures are the thief's too.
fn authorize_recovery(_offer: &RekeyOffer) -> Option<RecoveryAuthorization> {
    None
}

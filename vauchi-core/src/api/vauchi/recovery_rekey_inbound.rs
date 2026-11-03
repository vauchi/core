// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact taking in a rekey offer (vauchi/private#796, step 5a).
//!
//! An offer counts only when the new identity signed it and its recovery
//! proof opens for one of this device's contacts and names that same new
//! identity (`recovery_acceptance.spthy`, John_answers: the key his
//! accepted proof names). It is then kept for the contact to review with
//! the proof's verdict (D3); nothing changes until the contact confirms.

use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::exchange::X3DHKeyPair;
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::recovery::VerificationResult;
use crate::recovery::rekey::{RekeyAnswer, RekeyOffer, rekey_shared_key};

use super::Vauchi;
use super::recovery_rekey::RekeyDelivery;

/// An answer not yet followed by the new relationship goes again after a
/// day; its exchange key stays the one the contact already switched to.
const REKEY_ANSWER_RESEND_SECS: u64 = 86_400;

/// What [`Vauchi::take_rekey_offer`] made of an offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RekeyOfferTaken {
    /// Kept for review, from the contact with this id.
    Kept { contact_id: String },
    /// Not an offer this device can act on; nothing was kept.
    Dropped,
}

/// A kept rekey offer as the review lists it.
#[derive(Debug, Clone)]
pub struct PendingRekey {
    /// The recovered contact, under its old key.
    pub contact_id: String,
    /// The new identity the contact recovered to.
    pub new_pk: [u8; 32],
    /// What this device's contacts say about the recovery (B3).
    pub verdict: VerificationResult,
}

impl Vauchi {
    /// Takes in a rekey offer from the relay. Anything that is not a valid
    /// offer from a recovered contact is dropped without a trace, and so is
    /// every offer in a duress session (ADR-032).
    ///
    /// # Errors
    /// Storage errors only; a refused offer is [`RekeyOfferTaken::Dropped`].
    pub fn take_rekey_offer(&self, blob: &[u8]) -> VauchiResult<RekeyOfferTaken> {
        if self.identity.is_none() || self.in_duress_mode() {
            return Ok(RekeyOfferTaken::Dropped);
        }
        let Some((contact_id, _)) = self.checked_offer(blob)? else {
            return Ok(RekeyOfferTaken::Dropped);
        };
        self.storage
            .recovery()
            .keep_rekey_offer(&contact_id, blob)?;
        Ok(RekeyOfferTaken::Kept { contact_id })
    }

    /// The kept rekey offers whose proof still holds, for review.
    ///
    /// # Errors
    /// Storage errors.
    pub fn pending_rekey_offers(&self) -> VauchiResult<Vec<PendingRekey>> {
        let mut pending = Vec::new();
        for (_, blob) in self.storage.recovery().kept_rekey_offers()? {
            if let Some((contact_id, (new_pk, verdict))) = self.checked_offer(&blob)? {
                pending.push(PendingRekey {
                    contact_id,
                    new_pk,
                    verdict,
                });
            }
        }
        Ok(pending)
    }

    /// Accepts the kept rekey offer of `contact_id` (step 5b): answers it,
    /// moves the contact to the new key's id with the shared key both
    /// sides derive and a fresh ratchet, and keeps the answer to send.
    /// Returns the contact's new id.
    ///
    /// # Errors
    /// [`VauchiError::NotFound`] when no offer from `contact_id` is kept,
    /// [`VauchiError::InvalidState`] when it no longer holds; storage
    /// errors otherwise, which change nothing.
    pub fn accept_rekey_offer(&self, contact_id: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let blob = self
            .storage
            .recovery()
            .kept_rekey_offers()?
            .into_iter()
            .find(|(id, _)| id == contact_id)
            .map(|(_, blob)| blob)
            .ok_or_else(|| VauchiError::NotFound("No rekey offer from this contact".into()))?;
        let Some((checked_id, (new_pk, _))) = self.checked_offer(&blob)? else {
            return Err(VauchiError::InvalidState(
                "The rekey offer no longer holds".into(),
            ));
        };
        if checked_id != contact_id {
            return Err(VauchiError::InvalidState(
                "The rekey offer names another contact".into(),
            ));
        }
        let offer =
            RekeyOffer::decode(&blob).map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(contact_id.to_string()))?;
        let old_shared = contact
            .shared_key()
            .ok_or_else(|| VauchiError::InvalidState("The contact has no shared key".into()))?
            .clone();
        let exchange = X3DHKeyPair::generate();
        let shared = rekey_shared_key(
            &exchange,
            offer.exchange_key(),
            &new_pk,
            identity.signing_public_key(),
        )
        .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let answer = RekeyAnswer::create(identity, *exchange.public_key(), *offer.exchange_key());
        let ratchet = DoubleRatchetState::initialize_responder(&shared, exchange);
        let new_id = hex::encode(new_pk);
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage.move_contact_id(contact_id, &new_id)?;
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
            let recovery = self.storage.recovery();
            recovery.forget_rekey_offer(&new_id)?;
            recovery.keep_rekey_answer(&new_id, &answer.encode(), &old_shared)?;
            Ok(())
        })?;
        Ok(new_id)
    }

    /// Declines the kept rekey offer of `contact_id`: it is forgotten and
    /// nothing is answered.
    ///
    /// # Errors
    /// Storage errors.
    pub fn decline_rekey_offer(&self, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.recovery().forget_rekey_offer(contact_id)?)
    }

    /// The rekey answers due now, each to `token(old shared key, new key,
    /// day)`, the mailbox the recovered contact's new phone polls; one that
    /// went out in the last day waits.
    ///
    /// # Errors
    /// Storage errors.
    pub fn rekey_answer_deliveries(&self) -> VauchiResult<Vec<RekeyDelivery>> {
        let now = self.clock.unix_seconds();
        let day = current_day_epoch(now);
        let mut deliveries = Vec::new();
        for (contact_id, blob, old_shared, sent_at) in
            self.storage.recovery().kept_rekey_answers()?
        {
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

    /// The recovered contact's id, the new key and the proof's verdict
    /// when `blob` is a signed offer whose proof opens for a contact here
    /// and names the offer's identity; `None` otherwise.
    #[allow(clippy::type_complexity)]
    fn checked_offer(
        &self,
        blob: &[u8],
    ) -> VauchiResult<Option<(String, ([u8; 32], VerificationResult))>> {
        let Ok(offer) = RekeyOffer::decode(blob) else {
            return Ok(None);
        };
        if !offer.verify() {
            return Ok(None);
        }
        let Ok((proof, verdict)) = self.verify_recovery_proof(offer.proof()) else {
            return Ok(None);
        };
        if proof.new_pk() != offer.new_pk() {
            return Ok(None);
        }
        let contact_id = hex::encode(proof.old_pk());
        let Some(contact) = self.storage.contacts().load_contact(&contact_id)? else {
            return Ok(None);
        };
        if contact.is_blocked() || contact.public_key().is_none() {
            return Ok(None);
        }
        Ok(Some((contact_id, (*offer.new_pk(), verdict))))
    }
}

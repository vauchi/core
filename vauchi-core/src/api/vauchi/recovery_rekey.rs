// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Offering each restored contact a rekey (vauchi/private#796, step 4b).
//!
//! The new identity has a restored contact's old shared key and key, but
//! no ratchet with it. It reaches the contact where the contact already
//! looks for it: the mailbox token of the old relationship, addressed to
//! the contact's key. A thief holding the old device can compute that
//! mailbox too and drain it, which delays a rekey and cannot forge one.

use crate::api::error::{VauchiError, VauchiResult};
use crate::exchange::X3DHKeyPair;
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use crate::recovery::rekey::RekeyOffer;

use super::ProfileEngine;

/// An unanswered offer goes out again after a day, with the same key.
const REKEY_OFFER_RESEND_SECS: u64 = 86_400;

/// Offers and answers go out for this long, then stop (D18): a contact
/// silent that long is listed, and meeting in person settles it. The relay
/// keeps a blob for longer, and a device catches up 14 days per cycle.
pub(super) const REKEY_RESEND_WINDOW_SECS: u64 = 14 * 86_400;

/// Where a contact restored from the guardian backup stands in its rekey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RekeyWait {
    /// Offers still go to it.
    Asking,
    /// It did not answer within 14 days of the restore.
    NotAnswered,
}

/// What [`ProfileEngine::take_rekey_answer`] made of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RekeyAnswerTaken {
    /// The contact with this id moved to the new relationship.
    Completed { contact_id: String },
    /// Not an answer to an offer of this phone; nothing changed.
    Dropped,
}

/// One rekey offer due for a restored contact, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyDelivery {
    pub contact_id: String,
    pub token_hex: String,
    pub blob: Vec<u8>,
}

impl ProfileEngine {
    /// The rekey offers due now: one for each contact waiting for its rekey
    /// whose offer has not gone out in the last day. None before this
    /// identity holds a complete recovery proof, and none from a duress
    /// session (ADR-032).
    ///
    /// # Errors
    /// [`VauchiError::IdentityNotInitialized`] without an identity; storage
    /// errors otherwise.
    pub fn rekey_offer_deliveries(&self) -> VauchiResult<Vec<RekeyDelivery>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        if self.in_duress_mode() {
            return Ok(Vec::new());
        }
        let Some(proof) = self.storage.recovery().load_own_proof()? else {
            return Ok(Vec::new());
        };
        let now = self.clock.unix_seconds();
        let day = current_day_epoch(now);
        let mut deliveries = Vec::new();
        for (contact_id, restored_at) in self.storage.recovery().rekey_pending_since()? {
            if now >= restored_at.saturating_add(REKEY_RESEND_WINDOW_SECS) {
                continue;
            }
            let Some(contact) = self.storage.contacts().load_contact(&contact_id)? else {
                continue;
            };
            let (Some(contact_pk), Some(old_shared)) = (contact.public_key(), contact.shared_key())
            else {
                continue;
            };
            let exchange = match self.storage.recovery().rekey_exchange(&contact_id)? {
                Some((_, Some(sent_at)))
                    if now < sent_at.saturating_add(REKEY_OFFER_RESEND_SECS) =>
                {
                    continue;
                }
                Some((secret, _)) => X3DHKeyPair::from_bytes(*secret),
                None => {
                    let fresh = X3DHKeyPair::generate();
                    self.storage
                        .recovery()
                        .save_rekey_exchange(&contact_id, &fresh.secret_bytes())?;
                    fresh
                }
            };
            let offer = RekeyOffer::create(identity, *exchange.public_key(), proof.clone())
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            deliveries.push(RekeyDelivery {
                token_hex: token_hex(&compute_mailbox_token(
                    old_shared.as_bytes(),
                    contact_pk,
                    day,
                )),
                contact_id,
                blob: offer.encode(),
            });
        }
        Ok(deliveries)
    }

    /// Takes in a contact's answer to a rekey offer (step 6;
    /// `recovery_acceptance.spthy`, Alice_completes): only when the
    /// contact's key from the backup signed it and it names the exchange
    /// key this phone offered. Then the contact moves to the shared key both
    /// derived, with a fresh ratchet this phone starts, and this phone's
    /// card goes to it on the next sync. Anything else is dropped, and every
    /// answer in a duress session.
    ///
    /// # Errors
    /// Storage errors only, which change nothing.
    pub fn take_rekey_answer(&self, blob: &[u8]) -> VauchiResult<RekeyAnswerTaken> {
        let Some(identity) = self.identity.as_ref() else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        if self.in_duress_mode() {
            return Ok(RekeyAnswerTaken::Dropped);
        }
        let Ok(answer) = crate::recovery::rekey::RekeyAnswer::decode(blob) else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        let contact_id = hex::encode(answer.contact_pk());
        if !self
            .storage
            .recovery()
            .rekey_pending()?
            .contains(&contact_id)
        {
            return Ok(RekeyAnswerTaken::Dropped);
        }
        let Some((secret, _)) = self.storage.recovery().rekey_exchange(&contact_id)? else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        let exchange = X3DHKeyPair::from_bytes(*secret);
        let Some(mut contact) = self.storage.contacts().load_contact(&contact_id)? else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        let Some(contact_pk) = contact.public_key().copied() else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        if !answer.verify(&contact_pk, exchange.public_key()) {
            return Ok(RekeyAnswerTaken::Dropped);
        }
        let Ok(shared) = crate::recovery::rekey::rekey_shared_key(
            &exchange,
            answer.exchange_key(),
            identity.signing_public_key(),
            &contact_pk,
        ) else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        let Ok(ratchet) = crate::crypto::ratchet::DoubleRatchetState::initialize_initiator(
            &shared,
            *answer.exchange_key(),
        ) else {
            return Ok(RekeyAnswerTaken::Dropped);
        };
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            contact
                .adopt_exchange_shared_key(shared.clone(), now)
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            self.storage.contacts().save_contact(&contact)?;
            let ratchets = self.storage.ratchets();
            ratchets.delete_ratchet_states_for_contact(&contact_id)?;
            ratchets.save_ratchet_state(&contact_id, &ratchet, true)?;
            self.storage.recovery().clear_rekey_pending(&contact_id)?;
            Ok(())
        })?;
        self.record_rekey_settled(&contact_id)?;
        self.mark_own_card_repropagate()?;
        Ok(RekeyAnswerTaken::Completed { contact_id })
    }

    /// Each contact still waiting for its rekey, with where it stands, in
    /// the order restored, for the owner's list.
    ///
    /// # Errors
    /// Storage errors.
    pub fn rekey_waiting(&self) -> VauchiResult<Vec<(String, RekeyWait)>> {
        let now = self.clock.unix_seconds();
        Ok(self
            .storage
            .recovery()
            .rekey_pending_since()?
            .into_iter()
            .map(|(id, restored_at)| {
                let wait = if now >= restored_at.saturating_add(REKEY_RESEND_WINDOW_SECS) {
                    RekeyWait::NotAnswered
                } else {
                    RekeyWait::Asking
                };
                (id, wait)
            })
            .collect())
    }

    /// Records that a contact's rekey offer went out now, so it goes out
    /// again only after a day.
    ///
    /// # Errors
    /// Storage errors.
    pub fn record_rekey_offer_sent(&self, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .recovery()
            .record_rekey_offer_sent(contact_id, self.clock.unix_seconds())?)
    }

    /// Sends the rekey offers due now to the relay, recording each one that
    /// went out. Best-effort: a failed send is offered again next time.
    /// Returns how many went out.
    ///
    /// # Errors
    /// Storage errors while preparing the offers.
    #[cfg(feature = "network-http")]
    pub fn send_rekey_offers(&self) -> VauchiResult<usize> {
        use base64::Engine;
        if self.identity.is_none() {
            return Ok(0);
        }
        let deliveries = self.rekey_offer_deliveries()?;
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
                self.record_rekey_offer_sent(&delivery.contact_id)?;
                sent += 1;
            }
        }
        Ok(sent)
    }
}

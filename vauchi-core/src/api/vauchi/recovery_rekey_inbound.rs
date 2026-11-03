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

use crate::api::error::VauchiResult;
use crate::recovery::VerificationResult;
use crate::recovery::rekey::RekeyOffer;

use super::Vauchi;

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

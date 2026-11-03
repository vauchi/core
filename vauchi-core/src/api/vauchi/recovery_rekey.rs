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

use super::Vauchi;

/// An unanswered offer goes out again after a day, with the same key.
const REKEY_OFFER_RESEND_SECS: u64 = 86_400;

/// Offers and answers go out for this long, then stop (D18): a contact
/// silent that long is listed, and meeting in person settles it. The relay
/// keeps a blob for longer, and a device catches up 14 days per cycle.
pub(super) const REKEY_RESEND_WINDOW_SECS: u64 = 14 * 86_400;

/// One rekey offer due for a restored contact, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyDelivery {
    pub contact_id: String,
    pub token_hex: String,
    pub blob: Vec<u8>,
}

impl Vauchi {
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

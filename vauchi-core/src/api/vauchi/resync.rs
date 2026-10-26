// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The resync request (vauchi/private#691, maintainer 2026-10-26).
//!
//! The relay keeps an envelope 120 days. A client away longer has lost
//! what its contacts sent meanwhile, and a contact's card only converges
//! on that contact's next edit — so a catch-up that began behind the
//! retention horizon ends by asking every exchanged contact to
//! repropagate its visible card now. The request is ratchet-sealed like
//! any update; the receiver honors it at most once per contact per day,
//! and a blocked contact's request never reaches this layer.

use crate::network::mailbox_token::current_day_epoch;
use crate::sync::delta::encode_resync_request;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    /// Asks every exchanged contact this device can reach to repropagate
    /// its visible card. Returns how many contacts were asked; a contact
    /// with no session yet, like a blocked one, is skipped.
    pub fn request_card_resync(&self) -> VauchiResult<usize> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let mut asked = 0;
        for contact in self.storage.contacts().list_contacts()? {
            if !contact.is_exchanged() || contact.is_blocked() {
                continue;
            }
            match self.queue_sealed_payload(identity, &contact, &encode_resync_request()) {
                Ok(()) => asked += 1,
                // No session to send on yet: the contact's first message
                // re-arms the owed repropagation anyway.
                Err(VauchiError::SendDeferred)
                | Err(VauchiError::NotFound(_))
                | Err(VauchiError::InvalidState(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(asked)
    }

    /// Honors `contact_id`'s resync request: queues our visible card for
    /// it, at most once a day. Returns whether it was honored.
    pub fn honor_resync_request(&self, contact_id: &str) -> VauchiResult<bool> {
        let today = current_day_epoch(self.clock.unix_seconds());
        if self.storage.sync().load_resync_honored_day(contact_id)? == Some(today) {
            return Ok(false);
        }
        self.repropagate_or_defer(contact_id)?;
        self.storage
            .sync()
            .save_resync_honored_day(contact_id, today)?;
        Ok(true)
    }
}

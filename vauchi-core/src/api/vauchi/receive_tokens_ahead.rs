// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Each unlocked sync stores the next days' receive registrations under the
//! Background Receive Key, so a locked phone can fetch without a contact's
//! shared key, which also starts new ratchet sessions, or the master seed
//! (owner 2026-11-08, vauchi/private#1093 slice 5).

use crate::contact::Contact;
use crate::identity::Identity;
use crate::network::mailbox_token::{current_day_epoch, padded_registration_set};
use crate::storage::RegistrationDay;

use super::super::VauchiResult;
use super::ProfileEngine;
use super::receive_context::registered_mailboxes;

/// Matches the catch-up window: a phone locked longer than this since its
/// last unlocked sync stops receiving until the next one.
const DAYS_AHEAD: u64 = 14;

impl ProfileEngine {
    /// Best-effort: a failure leaves the days stored by an earlier sync,
    /// and the next sync retries.
    pub(super) fn store_receive_tokens_ahead(&self, identity: &Identity, contacts: &[Contact]) {
        if let Err(e) = self.try_store_receive_tokens_ahead(identity, contacts) {
            tracing::warn!(
                target: "vauchi.sync.receive_ahead",
                error = %e,
                "could not store the receive registrations ahead; the next sync retries"
            );
        }
    }

    fn try_store_receive_tokens_ahead(
        &self,
        identity: &Identity,
        contacts: &[Contact],
    ) -> VauchiResult<()> {
        if !self.storage.has_receive_key() {
            return Ok(());
        }
        let today = current_day_epoch(self.clock.unix_seconds());
        let days: Vec<(u64, RegistrationDay)> = (today..today + DAYS_AHEAD)
            .map(|day| {
                let context = registered_mailboxes(identity, contacts, &self.config.relay, &[day]);
                let tokens = context.routes.keys().cloned().collect();
                let batches = padded_registration_set(identity.master_seed(), day, tokens);
                (day, RegistrationDay::new(batches, context.routes))
            })
            .collect();
        let store = self.storage.receive_tokens();
        // Unchanged contacts give an unchanged today, so a sync within the
        // same day rewrites nothing.
        let today_unchanged = store.load_day(today)?.as_ref() == days.first().map(|(_, d)| d);
        if today_unchanged && store.load_day(today + DAYS_AHEAD - 1)?.is_some() {
            return Ok(());
        }
        store.replace_days(&days)?;
        Ok(())
    }
}

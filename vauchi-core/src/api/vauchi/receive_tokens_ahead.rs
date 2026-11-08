// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Each unlocked sync stores the next days' receive registrations under the
//! Background Receive Key, so a locked phone can fetch without a contact's
//! shared key, which also starts new ratchet sessions, or the master seed
//! (owner 2026-11-08, vauchi/private#1093 slice 5).

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::contact::Contact;
use crate::identity::Identity;
use crate::network::mailbox_context::MailboxRoute;
use crate::network::mailbox_token::{current_day_epoch, padded_registration_set};
use crate::storage::StorageError;

use super::super::VauchiResult;
use super::ProfileEngine;
use super::receive_context::registered_mailboxes;

/// Matches the catch-up window: a phone locked longer than this since its
/// last unlocked sync stops receiving until the next one.
const DAYS_AHEAD: u64 = 14;

/// One day's receive registration: the padded batches, before the shuffle,
/// and what each real token routes to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationDay {
    batches: Vec<Vec<String>>,
    routes: HashMap<String, MailboxRoute>,
}

impl RegistrationDay {
    /// The padded batches the day registers, each in a fixed order; a
    /// caller shuffles them before sending.
    pub fn batches(&self) -> &[Vec<String>] {
        &self.batches
    }

    /// The contacts the day's tokens receive from.
    pub fn contact_ids(&self) -> BTreeSet<&str> {
        self.routes
            .values()
            .filter_map(|route| match route {
                MailboxRoute::Contact { id, .. } => Some(id.as_str()),
                MailboxRoute::DeviceSync { .. } => None,
            })
            .collect()
    }
}

impl ProfileEngine {
    /// The receive registration stored ahead for `day`, if any.
    pub fn receive_registration_ahead(&self, day: u64) -> VauchiResult<Option<RegistrationDay>> {
        let Some(bytes) = self.storage.receive_tokens().load_day(day)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| StorageError::Serialization(e.to_string()).into())
    }

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
                (
                    day,
                    RegistrationDay {
                        batches,
                        routes: context.routes,
                    },
                )
            })
            .collect();
        // Unchanged contacts give an unchanged today, so a sync within the
        // same day rewrites nothing.
        let today_unchanged =
            self.receive_registration_ahead(today)?.as_ref() == days.first().map(|(_, d)| d);
        if today_unchanged
            && self
                .receive_registration_ahead(today + DAYS_AHEAD - 1)?
                .is_some()
        {
            return Ok(());
        }
        let sealed = days
            .iter()
            .map(|(day, registration)| {
                serde_json::to_vec(registration)
                    .map(|bytes| (*day, Zeroizing::new(bytes)))
                    .map_err(|e| StorageError::Serialization(e.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.storage.receive_tokens().replace_days(&sealed)?;
        Ok(())
    }
}

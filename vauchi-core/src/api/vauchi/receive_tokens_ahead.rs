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

/// An up-to-date receive registers yesterday and today (`plan_receive`),
/// so a locked run within the window below, on this day or the next, needs
/// yesterday, today and tomorrow.
const DAYS_BEFORE: u64 = 1;
const DAYS_AFTER: u64 = 1;

/// A locked phone receives for one day after its last unlocked sync, then
/// waits for the next unlock, whose catch-up fetches what it missed
/// (ADR-033 addendum 2026-11-09, point 3).
const LOCKED_RECEIVE_SECS: u64 = 86_400;

/// One day's receive registration: the padded batches, before the shuffle,
/// and what each real token routes to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrationDay {
    batches: Vec<Vec<String>>,
    routes: HashMap<String, MailboxRoute>,
    /// A day stored before the window existed reads as already ended.
    #[serde(default)]
    receive_until: u64,
}

impl RegistrationDay {
    /// The padded batches the day registers, each in a fixed order; a
    /// caller shuffles them before sending.
    pub fn batches(&self) -> &[Vec<String>] {
        &self.batches
    }

    /// Unix seconds after which a locked run no longer receives with this
    /// day.
    pub fn receive_until(&self) -> u64 {
        self.receive_until
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        serde_json::from_slice(bytes).map_err(|e| StorageError::Serialization(e.to_string()))
    }

    pub(crate) fn routes(&self) -> &HashMap<String, MailboxRoute> {
        &self.routes
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
        Ok(Some(RegistrationDay::decode(&bytes)?))
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
        let now = self.clock.unix_seconds();
        let today = current_day_epoch(now);
        let days: Vec<(u64, RegistrationDay)> = (today.saturating_sub(DAYS_BEFORE)
            ..=today + DAYS_AFTER)
            .map(|day| {
                let context = registered_mailboxes(identity, contacts, &self.config.relay, &[day]);
                let tokens = context.routes.keys().cloned().collect();
                let batches = padded_registration_set(identity.master_seed(), day, tokens);
                (
                    day,
                    RegistrationDay {
                        batches,
                        routes: context.routes,
                        receive_until: now + LOCKED_RECEIVE_SECS,
                    },
                )
            })
            .collect();
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

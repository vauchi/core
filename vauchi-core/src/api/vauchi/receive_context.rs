// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Capture local mailbox selectors and revalidate only their current
//! relationship keys when the corresponding request's answer is collected.

use std::collections::{HashMap, HashSet};

use crate::api::config::RelayConfig;
use crate::contact::Contact;
use crate::identity::Identity;
use crate::network::mailbox_context::{MailboxContext, MailboxRoute};
use crate::network::mailbox_token::{
    compute_device_mailbox_token, compute_device_sync_token, compute_mailbox_token, token_hex,
};

pub(super) fn registered_mailboxes(
    identity: &Identity,
    contacts: &[Contact],
    relay: &RelayConfig,
    days: &[u64],
) -> MailboxContext {
    let mut routes = HashMap::new();
    for &token_day in days {
        for contact in contacts
            .iter()
            .filter(|c| c.is_exchanged() && !c.is_blocked())
        {
            let Some(shared) = contact.shared_key() else {
                continue;
            };
            for device_scoped in [false, true] {
                let token = if device_scoped {
                    compute_device_mailbox_token(
                        shared.as_bytes(),
                        identity.signing_public_key(),
                        identity.device_id(),
                        token_day,
                    )
                } else {
                    compute_mailbox_token(
                        shared.as_bytes(),
                        identity.signing_public_key(),
                        token_day,
                    )
                };
                routes.insert(
                    token_hex(&token),
                    MailboxRoute::Contact {
                        id: contact.id().to_string(),
                        day: token_day,
                        device_scoped,
                    },
                );
            }
        }
        let token =
            compute_device_sync_token(identity.master_seed(), identity.device_id(), token_day);
        routes.insert(
            token_hex(&token),
            MailboxRoute::DeviceSync { day: token_day },
        );
    }
    MailboxContext {
        identity: *identity.signing_public_key(),
        device: *identity.device_id(),
        application_relay: relay.server_url.clone(),
        ohttp_relay: relay.ohttp_endpoint(),
        routes,
    }
}

/// The padded, shuffled registration batches for a set of routes.
pub(super) fn registration_batches(
    rng: &dyn crate::rng::SecureRng,
    routes: &HashMap<String, MailboxRoute>,
) -> Vec<Vec<String>> {
    crate::network::mailbox_token::padded_registration_batches(
        rng,
        routes.keys().cloned().collect(),
    )
}

pub(super) struct ReceiveContext<'a> {
    identity: &'a Identity,
    contacts: HashMap<&'a str, &'a Contact>,
    relay: &'a RelayConfig,
    pub(super) contact_tokens: HashMap<String, String>,
    pub(super) self_tokens: HashSet<String>,
}

impl<'a> ReceiveContext<'a> {
    pub(super) fn new(
        identity: &'a Identity,
        contacts: &'a [Contact],
        relay: &'a RelayConfig,
    ) -> Self {
        Self {
            identity,
            contacts: contacts.iter().map(|c| (c.id(), c)).collect(),
            relay,
            contact_tokens: HashMap::new(),
            self_tokens: HashSet::new(),
        }
    }

    pub(super) fn include(&mut self, context: &MailboxContext, token: &str) -> bool {
        if context.identity != *self.identity.signing_public_key()
            || context.device != *self.identity.device_id()
            || context.application_relay != self.relay.server_url
            || context.ohttp_relay != self.relay.ohttp_endpoint()
        {
            return false;
        }
        match context.routes.get(token) {
            Some(MailboxRoute::Contact {
                id,
                day,
                device_scoped,
            }) => {
                let Some(contact) = self.contacts.get(id.as_str()) else {
                    return false;
                };
                if !contact.is_exchanged() || contact.is_blocked() {
                    return false;
                }
                let Some(shared) = contact.shared_key() else {
                    return false;
                };
                let expected = if *device_scoped {
                    compute_device_mailbox_token(
                        shared.as_bytes(),
                        self.identity.signing_public_key(),
                        self.identity.device_id(),
                        *day,
                    )
                } else {
                    compute_mailbox_token(
                        shared.as_bytes(),
                        self.identity.signing_public_key(),
                        *day,
                    )
                };
                if token_hex(&expected) != token {
                    return false;
                }
                self.contact_tokens.insert(token.to_string(), id.clone());
                true
            }
            Some(MailboxRoute::DeviceSync { day }) => {
                let expected = compute_device_sync_token(
                    self.identity.master_seed(),
                    self.identity.device_id(),
                    *day,
                );
                if token_hex(&expected) != token {
                    return false;
                }
                self.self_tokens.insert(token.to_string());
                true
            }
            None => false,
        }
    }
}

// INLINE_TEST_REQUIRED: request context is crate-private authorization state;
// exercise its API without exporting it to application shells.
#[cfg(test)]
#[path = "receive_context_tests.rs"]
mod tests;

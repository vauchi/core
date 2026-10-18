// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Revalidate locally captured fetch windows against the current identity,
//! endpoints and relationship keys before attributing a collected response.

use std::collections::{HashMap, HashSet};

use super::device_sync_loop::self_token_hexes;
use super::receive_routing::build_token_to_contact_map;
use crate::api::config::RelayConfig;
use crate::contact::Contact;
use crate::identity::Identity;
use crate::network::mailbox_context::MailboxContext;

pub(super) struct ReceiveContext<'a> {
    identity: &'a Identity,
    contacts: &'a [Contact],
    relay: &'a RelayConfig,
    days: HashSet<u64>,
    pub(super) contact_tokens: HashMap<String, String>,
    pub(super) self_tokens: HashSet<String>,
}

// INLINE_TEST_REQUIRED: request context is crate-private authorization state;
// exercise its API without exporting it to application shells.
#[cfg(test)]
#[path = "receive_context_tests.rs"]
mod tests;

impl<'a> ReceiveContext<'a> {
    pub(super) fn new(
        identity: &'a Identity,
        contacts: &'a [Contact],
        relay: &'a RelayConfig,
    ) -> Self {
        Self {
            identity,
            contacts,
            relay,
            days: HashSet::new(),
            contact_tokens: HashMap::new(),
            self_tokens: HashSet::new(),
        }
    }

    pub(super) fn include(&mut self, context: &MailboxContext) -> bool {
        if context.identity != *self.identity.signing_public_key()
            || context.device != *self.identity.device_id()
            || context.application_relay != self.relay.server_url
            || context.ohttp_relay != self.relay.ohttp_endpoint()
        {
            return false;
        }
        if self.days.insert(context.day) {
            self.contact_tokens.extend(build_token_to_contact_map(
                self.contacts,
                self.identity.signing_public_key(),
                self.identity.device_id(),
                context.day,
            ));
            self.self_tokens
                .extend(self_token_hexes(self.identity, context.day));
        }
        true
    }
}

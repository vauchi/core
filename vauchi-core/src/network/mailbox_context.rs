// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local provenance for a fetch's registered daily mailbox window.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) enum MailboxRoute {
    Contact {
        id: String,
        day: u64,
        device_scoped: bool,
    },
    DeviceSync {
        day: u64,
    },
}

#[derive(Clone)]
pub(crate) struct MailboxContext {
    pub(crate) identity: [u8; 32],
    pub(crate) device: [u8; 32],
    pub(crate) application_relay: String,
    pub(crate) ohttp_relay: String,
    pub(crate) routes: HashMap<String, MailboxRoute>,
}

impl MailboxContext {
    pub(crate) fn for_tokens(&self, tokens: &[String]) -> Self {
        Self {
            identity: self.identity,
            device: self.device,
            application_relay: self.application_relay.clone(),
            ohttp_relay: self.ohttp_relay.clone(),
            routes: tokens
                .iter()
                .filter_map(|token| {
                    self.routes
                        .get(token)
                        .map(|route| (token.clone(), route.clone()))
                })
                .collect(),
        }
    }
}

impl std::fmt::Debug for MailboxContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MailboxContext")
            .field("route_count", &self.routes.len())
            .finish_non_exhaustive()
    }
}

// INLINE_TEST_REQUIRED: this crate-private request context must not be
// exported to shells merely to verify its bounded subset contract.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal: A pending request retains routes for its own tokens only.
    #[test]
    fn a_request_does_not_keep_unrequested_routes() {
        let first = "aa".repeat(32);
        let second = "bb".repeat(32);
        let full = MailboxContext {
            identity: [1; 32],
            device: [2; 32],
            application_relay: "https://gateway.invalid".into(),
            ohttp_relay: "https://forwarder.invalid".into(),
            routes: HashMap::from([
                (first.clone(), MailboxRoute::DeviceSync { day: 0 }),
                (second.clone(), MailboxRoute::DeviceSync { day: 1 }),
            ]),
        };
        let request = full.for_tokens(&[first.clone(), "cc".repeat(32)]);
        assert!(matches!(
            request.routes.get(&first),
            Some(MailboxRoute::DeviceSync { day: 0 })
        ));
        assert!(request.routes.get(&second).is_none());
        assert_eq!(request.routes.len(), 1);
    }
}

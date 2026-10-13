// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode exchange as a domain API (private#670): share a link or accept
//! one, then advance until the exchange completes. Lets a shell without the
//! presentation protocol (the CLI) run the same ceremony the apps run.

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};

/// A link-mode exchange in progress on this device.
pub struct LinkExchange {
    _private: (),
}

/// Where a link-mode exchange stands after an advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkExchangeProgress {
    /// Still waiting for the other side or the relay.
    Waiting,
    /// Both cards arrived; the contact exists.
    Completed { contact_id: String },
    /// The exchange ended without a contact (a stable failure id).
    Failed { reason: String },
}

impl Vauchi {
    /// Start a link-mode exchange as the initiator: returns the link to send
    /// and the exchange to advance.
    pub fn start_link_exchange(&self) -> VauchiResult<(String, LinkExchange)> {
        Err(VauchiError::InvalidState("not implemented".into()))
    }

    /// Accept another person's exchange link as the responder.
    pub fn accept_link_exchange(&self, _link: &str) -> VauchiResult<LinkExchange> {
        Err(VauchiError::InvalidState("not implemented".into()))
    }

    /// Run the exchange's pending relay steps once and report progress.
    pub fn advance_link_exchange(
        &self,
        _exchange: &mut LinkExchange,
    ) -> VauchiResult<LinkExchangeProgress> {
        Ok(LinkExchangeProgress::Waiting)
    }
}

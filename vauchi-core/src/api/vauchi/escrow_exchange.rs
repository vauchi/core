// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay-escrow exchange — `Vauchi::escrow_exchange` (ADR-049).
//!
//! Core performs the relay escrow round-trip itself rather than emitting a
//! frontend `RelayEscrow*` command (which no frontend ever executed — see
//! `_private/docs/problems/2026-06-02-relay-escrow-execution-gap/`). This
//! is the network primitive the link-mode and reciprocity poll drivers
//! call; it builds the same OHTTP-configured relay transport that sync and
//! code-based relay exchange use.

use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse};

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};

impl Vauchi {
    /// Perform a single relay escrow round-trip over the configured relay
    /// transport.
    ///
    /// Routes over OHTTP when an OHTTP key is cached (ADR-037 privacy),
    /// falling back to direct HTTP only when none is configured — mirroring
    /// `create_relay_transport` and the per-action call pattern used by
    /// code-based relay exchange. Network failures surface as
    /// [`VauchiError::Network`].
    pub fn escrow_exchange(&self, message: &EscrowMessage) -> VauchiResult<EscrowResponse> {
        let transport =
            self.build_relay_transport(self.http_relay_url(), self.config.relay.connect_timeout_ms);
        transport.escrow(message).map_err(VauchiError::Network)
    }
}

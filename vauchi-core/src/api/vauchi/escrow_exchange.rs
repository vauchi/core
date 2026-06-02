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
use crate::network::escrow_client::{EscrowOutcome, escrow_outcome, escrow_request};
use crate::platform::{Command, Event};

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

    /// Execute one escrow `Command` over the relay and return the matching
    /// hardware [`Event`] (paired with the command's gate hash) to feed
    /// back into the link-mode / reciprocity state machine — the
    /// core-owned replacement for the frontend executing the command and
    /// reporting the event (ADR-049).
    ///
    /// Returns `None` for a non-escrow command, a deposit ack, a still-
    /// pending poll, or a transient network error (the caller re-polls on
    /// the next tick; a persistent failure is caught by the machine's
    /// polling deadline).
    pub fn run_escrow_command(&self, command: &Command) -> Option<Event> {
        let gate_hash = match command {
            Command::RelayEscrowDeposit { gate_hash, .. }
            | Command::RelayEscrowCheck { gate_hash, .. }
            | Command::RelayEscrowRetrieve { gate_hash, .. } => gate_hash.clone(),
            _ => return None,
        };
        let message = escrow_request(command)?;
        let outcome = escrow_outcome(&self.escrow_exchange(&message).ok()?);
        match outcome {
            EscrowOutcome::Deposited | EscrowOutcome::Pending => None,
            EscrowOutcome::Ready => Some(Event::RelayEscrowReady { gate_hash }),
            EscrowOutcome::Retrieved(blob) => {
                Some(Event::RelayEscrowBlobReceived { gate_hash, blob })
            }
            EscrowOutcome::Failed(reason) => Some(Event::RelayEscrowFailed { gate_hash, reason }),
        }
    }
}

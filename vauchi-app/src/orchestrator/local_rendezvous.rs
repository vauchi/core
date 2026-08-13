// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Single-ceremony rendezvous for local device linking (ADR-070 Phase 1).
//!
//! The relay is a third party that mints a code and holds a payload until
//! the peer claims it. A LAN has no third party, so the QR-displaying
//! device hosts the rendezvous itself — it already minted the ceremony and
//! already sees the peer's payload by virtue of being the peer, so hosting
//! adds no exposure.
//!
//! One instance serves exactly one ceremony. That is the whole security
//! posture: there is no code space to enumerate, no second ceremony to
//! confuse it with, and nothing to garbage-collect.
//!
//! Payloads are opaque base64 (ADR-004). This type never interprets them,
//! which is what lets a non-relay broker be safe at all.
//!
//! The code is supplied rather than minted here so the type stays pure and
//! deterministic under test; callers mint it from the app's secure RNG.

use std::sync::Mutex;

/// Why a rendezvous operation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RendezvousError {
    /// A ceremony is already offered — one instance serves exactly one.
    #[error("rendezvous already holds a ceremony")]
    AlreadyOffered,

    /// No ceremony is offered, or the code does not match the one held.
    #[error("unknown rendezvous code")]
    UnknownCode,

    /// A peer already claimed this ceremony.
    #[error("rendezvous already claimed")]
    AlreadyClaimed,
}

struct Ceremony {
    code: String,
    offered: String,
    response: Option<String>,
}

/// Host-side state for one local device-link ceremony.
#[derive(Default)]
pub struct SingleCeremonyRendezvous {
    ceremony: Mutex<Option<Ceremony>>,
}

impl SingleCeremonyRendezvous {
    /// Empty rendezvous, holding no ceremony.
    pub fn new() -> Self {
        Self::default()
    }

    /// Post the offered payload under `code`. Mirrors `exchange_offer`.
    pub fn offer(&self, code: String, payload: String) -> Result<(), RendezvousError> {
        let mut held = self.held();
        if held.is_some() {
            return Err(RendezvousError::AlreadyOffered);
        }
        *held = Some(Ceremony {
            code,
            offered: payload,
            response: None,
        });
        Ok(())
    }

    /// Deposit `response` and return the offered payload. Mirrors
    /// `exchange_claim`.
    ///
    /// A second claim is refused rather than allowed to overwrite: once
    /// this is on a socket, two peers racing is reachable, and the winner's
    /// response must survive the loser.
    pub fn claim(&self, code: &str, response: &str) -> Result<String, RendezvousError> {
        let mut held = self.held();
        let ceremony = held
            .as_mut()
            .filter(|c| c.code == code)
            .ok_or(RendezvousError::UnknownCode)?;
        if ceremony.response.is_some() {
            return Err(RendezvousError::AlreadyClaimed);
        }
        ceremony.response = Some(response.to_string());
        Ok(ceremony.offered.clone())
    }

    /// Single-shot poll: `Ok(None)` until a peer has claimed. Mirrors
    /// `exchange_complete`.
    ///
    /// Repeatable by design — the machine polls once per `advance()`, so a
    /// consuming read would lose the response on the following tick.
    pub fn complete(&self, code: &str) -> Result<Option<String>, RendezvousError> {
        let held = self.held();
        let ceremony = held
            .as_ref()
            .filter(|c| c.code == code)
            .ok_or(RendezvousError::UnknownCode)?;
        Ok(ceremony.response.clone())
    }

    /// Nothing inside the lock can panic — only string moves and
    /// comparisons — so poisoning would mean a bug elsewhere. Recovering
    /// keeps a network-reachable path from turning that into a hard
    /// failure (DC-01: fail closed, not loudly).
    fn held(&self) -> std::sync::MutexGuard<'_, Option<Ceremony>> {
        self.ceremony
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

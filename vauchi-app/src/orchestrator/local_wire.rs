// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire format between a local device-link joiner and the host rendezvous
//! (ADR-070 Phase 1).
//!
//! One request per connection: read a bounded frame, answer it, close.
//! There is no framing state to get wrong, no session to resume, and a
//! stalled peer costs one connection rather than a stuck protocol — the
//! machine already drives `exchange_complete` as a repeated single-shot
//! poll, so request/response per connection matches how it behaves.
//!
//! Carries no transport-level encryption (ADR-070 Open Question 3): the
//! payloads are opaque base64 the rendezvous never interprets (ADR-004),
//! so the socket holds no secret. The accepted residual is that a
//! same-segment observer sees that a ceremony happened, with timing and
//! sizes, but not what was exchanged.
//!
//! Everything here is reachable by anyone on the segment, so every input is
//! hostile until bounded (DC-01). Limits mirror the relay's own so a local
//! ceremony accepts exactly what a relay one does.

use serde::{Deserialize, Serialize};

use crate::orchestrator::local_rendezvous::{RendezvousError, SingleCeremonyRendezvous};

/// Largest frame read from a connection, mirroring the relay's
/// `DefaultBodyLimit`. Applied before parsing, so an oversized frame costs
/// a length check rather than an allocation.
pub const MAX_FRAME_BYTES: usize = 128 * 1024;

/// Largest exchange payload, mirroring the relay's
/// `MAX_EXCHANGE_PAYLOAD_BYTES`. A local ceremony must not accept what a
/// relay one would refuse.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Longest rendezvous code accepted. Codes are Core-minted and short; this
/// only stops an unbounded string being held for comparison.
pub const MAX_CODE_BYTES: usize = 64;

/// Why a frame was refused before it reached the rendezvous.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The frame exceeded [`MAX_FRAME_BYTES`].
    #[error("frame too large")]
    FrameTooLarge,

    /// The frame was not the expected JSON shape.
    #[error("malformed request")]
    Malformed,

    /// A bounded field exceeded its limit.
    #[error("field too large")]
    FieldTooLarge,
}

/// A request from the joiner. Mirrors the three `DeviceLinkBroker` calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LocalRequest {
    /// Post an offer; the host answers with the code it is served under.
    ExchangeOffer {
        /// Opaque base64 payload.
        payload: String,
    },
    /// Claim `code`, depositing `response`, and receive the offered payload.
    ExchangeClaim {
        /// Rendezvous code being claimed.
        code: String,
        /// Opaque base64 payload deposited for the offerer.
        response: String,
    },
    /// Single-shot poll for whether `code` has been claimed.
    ExchangeComplete {
        /// Rendezvous code being polled.
        code: String,
    },
}

/// The host's answer. `status` mirrors the relay's `{status, …}` shape so
/// the two brokers stay legible side by side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LocalResponse {
    /// An offer was accepted and is served under `code`.
    Offered {
        /// The code the offer is served under.
        code: String,
    },
    /// A claim succeeded; `payload` is what the offerer posted.
    Claimed {
        /// The offerer's opaque payload.
        payload: String,
    },
    /// A poll answered: `Some` once claimed, `None` while pending.
    Polled {
        /// The claimant's response, once one exists.
        response: Option<String>,
    },
    /// The request was refused.
    Error {
        /// Refusal reason, deliberately coarse.
        error: String,
    },
}

/// Parse a frame, enforcing every bound before the rendezvous sees it.
///
/// Length is checked before parsing, so an oversized frame costs a
/// comparison rather than an allocation (DC-01).
pub fn decode_request(frame: &[u8]) -> Result<LocalRequest, WireError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    let request: LocalRequest = serde_json::from_slice(frame).map_err(|_| WireError::Malformed)?;

    let within = match &request {
        LocalRequest::ExchangeOffer { payload } => payload.len() <= MAX_PAYLOAD_BYTES,
        LocalRequest::ExchangeClaim { code, response } => {
            code.len() <= MAX_CODE_BYTES && response.len() <= MAX_PAYLOAD_BYTES
        }
        LocalRequest::ExchangeComplete { code } => code.len() <= MAX_CODE_BYTES,
    };
    if !within {
        return Err(WireError::FieldTooLarge);
    }
    Ok(request)
}

/// Answer a request from `rendezvous`.
///
/// `minted_code` is consumed only by the offer arm, where it becomes the
/// code the offer is served under — the host issues it, mirroring the relay,
/// whose `exchange_offer` returns the code rather than accepting one.
///
/// **It must carry full CSPRNG entropy.** `device_link_responder_machine.rs`
/// records that unguessability rests on the issuer, and the relay backs its
/// six digits with per-code and global claim rate limits. A local host has
/// no such limiter, so the entropy has to do that work alone; six digits
/// here would be brute-forceable by anyone on the segment.
pub fn serve(
    rendezvous: &SingleCeremonyRendezvous,
    request: LocalRequest,
    minted_code: &str,
) -> LocalResponse {
    match request {
        LocalRequest::ExchangeOffer { payload } => {
            match rendezvous.offer(minted_code.to_string(), payload) {
                Ok(()) => LocalResponse::Offered {
                    code: minted_code.to_string(),
                },
                Err(e) => refusal(e),
            }
        }
        LocalRequest::ExchangeClaim { code, response } => {
            match rendezvous.claim(&code, &response) {
                Ok(payload) => LocalResponse::Claimed { payload },
                Err(e) => refusal(e),
            }
        }
        LocalRequest::ExchangeComplete { code } => match rendezvous.complete(&code) {
            Ok(response) => LocalResponse::Polled { response },
            Err(e) => refusal(e),
        },
    }
}

/// Encode a response for the wire.
pub fn encode_response(response: &LocalResponse) -> Vec<u8> {
    serde_json::to_vec(response)
        .unwrap_or_else(|_| b"{\"status\":\"error\",\"error\":\"encode\"}".to_vec())
}

/// Refusals are coarse on purpose: a peer on the segment learns that its
/// request was refused, never which ceremony state produced that.
fn refusal(_e: RendezvousError) -> LocalResponse {
    LocalResponse::Error {
        error: "refused".into(),
    }
}

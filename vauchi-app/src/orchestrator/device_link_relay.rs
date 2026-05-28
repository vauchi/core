// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link relay broker abstraction.
//!
//! Hosts the [`DeviceLinkBroker`] trait — the object-safe rendezvous
//! surface the device-link state machines drive one non-blocking step
//! at a time (slice 32l Phase 1) — and the [`ClaimPayload`] type the
//! responder deposits on the broker.
//!
//! The legacy single-call relay fns (`listen_for_request`,
//! `create_offer_and_listen`, `poll_for_claim`, `send_and_receive`,
//! `poll_for_response`) and their supporting types (`create_offer`,
//! `claim_and_send_request`, `send_response`, `DeviceLinkRelayMessage`,
//! `DeviceLinkError`) were retired with slice 32l once the
//! `DeviceLinkBroker`-driven machine in `device_link_machine.rs`
//! superseded them. The retirement zeroed four residual `Instant::now`
//! ratchet sites that the legacy poll-and-sleep cycle owned.

use vauchi_core::network::{HttpTransport, NetworkError};

/// Relay-broker rendezvous operations the device-link state machines
/// need (slice 32l Phase 1; design
/// `_private/docs/designs/2026-05-24-slice-32l-phase-1-device-link-state-machine-design.md`).
///
/// Abstracts the three `HttpTransport` exchange_* methods behind an
/// object-safe trait so the initiator/responder machines can be
/// advanced **one non-blocking step at a time** and driven by a fake
/// in tests (R1 seam). `exchange_complete` is the single-shot poll
/// primitive: `Ok(None)` means "not yet claimed" — the machine calls
/// it once per `advance()`.
pub trait DeviceLinkBroker {
    /// Post an offer payload; returns the broker code (embedded in the QR).
    fn exchange_offer(
        &self,
        payload_b64: &str,
        expires_secs: Option<u64>,
    ) -> Result<String, NetworkError>;

    /// Claim a code, depositing our payload; returns the peer's payload.
    fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError>;

    /// Single-shot, non-blocking poll. `Ok(None)` = not yet claimed.
    fn exchange_complete(&self, code: &str) -> Result<Option<String>, NetworkError>;
}

impl DeviceLinkBroker for HttpTransport {
    fn exchange_offer(
        &self,
        payload_b64: &str,
        expires_secs: Option<u64>,
    ) -> Result<String, NetworkError> {
        HttpTransport::exchange_offer(self, payload_b64, expires_secs)
    }

    fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
        HttpTransport::exchange_claim(self, code, response_b64)
    }

    fn exchange_complete(&self, code: &str) -> Result<Option<String>, NetworkError> {
        HttpTransport::exchange_complete(self, code)
    }
}

/// Payload sent by the new device in the claim step.
/// Contains the encrypted request and a response_code for the return channel.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct ClaimPayload {
    pub(crate) request: Vec<u8>,
    pub(crate) response_code: String,
}

// INLINE_TEST_REQUIRED: ClaimPayload is pub(crate), cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::*;

    // @scenario: device_sync:Claim payload serialization roundtrip
    #[test]
    fn test_claim_payload_roundtrip() {
        let payload = ClaimPayload {
            request: vec![1, 2, 3],
            response_code: "ABC123".to_string(),
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let parsed: ClaimPayload = serde_json::from_slice(&json).unwrap();
        assert_eq!(parsed.request, vec![1, 2, 3]);
        assert_eq!(parsed.response_code, "ABC123");
    }
}

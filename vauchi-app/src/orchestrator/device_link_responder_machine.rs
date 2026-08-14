// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **responder** (join) state machine — M5 B3 Slice 2.
//!
//! The mirror of [`super::device_link_machine::DeviceLinkInitiatorMachine`]
//! for the *new* device that receives a [`DeviceLinkJoinInvitation`] to join
//! an existing identity. Poll-driven over the same [`DeviceLinkBroker`]
//! rendezvous seam so it can be advanced one non-blocking step at a time and
//! driven by a fake in tests (real crypto throughout — ADR-002, no mocking).
//!
//! Flow: `Scanning` → post the encrypted request on the broker (claim the
//! initiator's rendezvous code, deposit our request + a return-channel
//! `response_code`) → `AwaitingResponse` → poll the return channel until
//! the initiator posts the encrypted response → decrypt it into a
//! [`DeviceLinkResponse`] the caller adopts via
//! `Vauchi::adopt_device_link_response` (Slice 1).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use vauchi_core::exchange::{
    DeviceLinkJoinInvitation, DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse, ExchangeError,
};

use super::device_link_relay::{ClaimPayload, DeviceLinkBroker};

/// Errors that prevent the responder machine from starting.
#[derive(Debug, thiserror::Error)]
pub enum ResponderMachineError {
    /// The invitation URL could not be parsed.
    #[error("invalid invitation: {0}")]
    InvalidInvitation(#[from] vauchi_core::exchange::JoinInvitationError),
    /// The embedded QR data is expired or malformed.
    #[error("invalid QR: {0}")]
    InvalidQr(#[from] ExchangeError),
}

/// Observable phase of the responder machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponderPhase {
    /// Constructed with an invitation; no relay I/O yet.
    Scanning,
    /// Request posted; polling the return channel for the response.
    AwaitingResponse,
    /// Response decrypted and ready to adopt.
    Completed,
    Failed,
    Cancelled,
}

/// What a transition produced. `DeviceLinkResponse` is deliberately not
/// `Clone` (it holds the master seed), so the response is retrieved via
/// [`DeviceLinkResponderMachine::take_response`] rather than carried here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponderEvent {
    /// No observable change this step (e.g. response not yet posted).
    None,
    /// Request posted to the relay. `confirmation_code` matches the code
    /// the initiator shows, so the user can verify the two devices agree.
    RequestPosted { confirmation_code: String },
    /// The response is decrypted and ready — retrieve it with
    /// [`DeviceLinkResponderMachine::take_response`] and adopt it.
    ResponseReady,
    /// Terminal failure (stable reason ids: `qr_expired`, `decrypt_error`,
    /// `invalid_qr`, `relay_failed`, `link_failed`).
    Failed { reason: String },
}

/// Internal state carrying the owned data each phase needs.
enum State {
    Scanning {
        qr: DeviceLinkQR,
        broker_code: String,
        device_name: String,
    },
    AwaitingResponse {
        responder: DeviceLinkResponder,
        response_code: String,
        deadline_unix: u64,
    },
    Completed,
    Failed,
    Cancelled,
}

/// Deterministic, poll-driven device-link responder.
pub struct DeviceLinkResponderMachine {
    state: State,
    relay_timeout_secs: u64,
    response: Option<DeviceLinkResponse>,
}

impl DeviceLinkResponderMachine {
    /// Construct in `Scanning` from a parsed or received
    /// [`DeviceLinkJoinInvitation`]. **No relay I/O** — the request is
    /// posted on the first [`advance`](Self::advance).
    pub fn new(
        invitation: DeviceLinkJoinInvitation,
        device_name: String,
        relay_timeout_secs: u64,
    ) -> Result<Self, ResponderMachineError> {
        let qr = DeviceLinkQR::from_data_string(&invitation.qr_data)?;
        Ok(Self {
            state: State::Scanning {
                qr,
                broker_code: invitation.broker_code,
                device_name,
            },
            relay_timeout_secs,
            response: None,
        })
    }

    /// Current observable phase.
    pub fn phase(&self) -> ResponderPhase {
        match &self.state {
            State::Scanning { .. } => ResponderPhase::Scanning,
            State::AwaitingResponse { .. } => ResponderPhase::AwaitingResponse,
            State::Completed => ResponderPhase::Completed,
            State::Failed => ResponderPhase::Failed,
            State::Cancelled => ResponderPhase::Cancelled,
        }
    }

    /// One non-blocking relay step. In `Scanning`: build + post the
    /// request → `RequestPosted`. In `AwaitingResponse`: poll the return
    /// channel once → `ResponseReady`, `None` (not yet), or
    /// `Failed("qr_expired")` once `now >= deadline`.
    pub fn advance(&mut self, broker: &dyn DeviceLinkBroker, now: u64) -> ResponderEvent {
        // `State::Failed` is a transient placeholder; every arm reassigns
        // `self.state` before returning.
        match std::mem::replace(&mut self.state, State::Failed) {
            State::Scanning {
                qr,
                broker_code,
                device_name,
            } => self.post_request(broker, qr, broker_code, device_name, now),
            State::AwaitingResponse {
                responder,
                response_code,
                deadline_unix,
            } => {
                if now >= deadline_unix {
                    self.state = State::Failed;
                    return ResponderEvent::Failed {
                        reason: "qr_expired".to_string(),
                    };
                }
                match broker.exchange_complete(&response_code) {
                    Ok(Some(response_b64)) => self.on_response(responder, &response_b64),
                    Ok(None) => {
                        self.state = State::AwaitingResponse {
                            responder,
                            response_code,
                            deadline_unix,
                        };
                        ResponderEvent::None
                    }
                    Err(_) => {
                        self.state = State::Failed;
                        ResponderEvent::Failed {
                            reason: "relay_failed".to_string(),
                        }
                    }
                }
            }
            terminal => {
                self.state = terminal;
                ResponderEvent::None
            }
        }
    }

    /// Build the encrypted request, deposit it on the broker under the
    /// initiator's rendezvous code with a fresh return-channel code, and
    /// move to `AwaitingResponse`.
    fn post_request(
        &mut self,
        broker: &dyn DeviceLinkBroker,
        qr: DeviceLinkQR,
        broker_code: String,
        device_name: String,
        now: u64,
    ) -> ResponderEvent {
        let mut responder = match DeviceLinkResponder::from_qr(qr, device_name, now) {
            Ok(r) => r,
            Err(e) => return self.fail(&e),
        };
        let request = match responder.create_request(now) {
            Ok(r) => r,
            Err(e) => return self.fail(&e),
        };
        let confirmation_code = match responder.compute_confirmation_code() {
            Ok(c) => c,
            Err(e) => return self.fail(&e),
        };

        // Return channel the initiator posts the response to. It must be
        // *minted by the broker*, not invented here: the relay only
        // accepts six-digit codes it issued, so a locally generated one is
        // refused at `exchange_claim` — after the user has already
        // confirmed, which is the worst moment to fail
        // (2026-08-14-device-link-host-never-shows-the-confirmation-code).
        // Offering also opens the slot, which a rendezvous that does not
        // create slots on claim requires (ADR-070's local broker).
        // Unguessability now rests on the relay's issuance and its
        // per-code and global claim rate limits, not on our entropy.
        let response_code = match broker.exchange_offer("", Some(self.relay_timeout_secs)) {
            Ok(code) => code,
            Err(_) => {
                self.state = State::Failed;
                return ResponderEvent::Failed {
                    reason: "relay_failed".to_string(),
                };
            }
        };
        let claim = ClaimPayload {
            request,
            response_code: response_code.clone(),
        };
        let claim_b64 =
            BASE64.encode(serde_json::to_vec(&claim).expect("ClaimPayload always serializes"));
        if broker.exchange_claim(&broker_code, &claim_b64).is_err() {
            self.state = State::Failed;
            return ResponderEvent::Failed {
                reason: "relay_failed".to_string(),
            };
        }

        self.state = State::AwaitingResponse {
            responder,
            response_code,
            deadline_unix: now.saturating_add(self.relay_timeout_secs),
        };
        ResponderEvent::RequestPosted { confirmation_code }
    }

    /// Decrypt the initiator's response into a `DeviceLinkResponse`.
    fn on_response(
        &mut self,
        responder: DeviceLinkResponder,
        response_b64: &str,
    ) -> ResponderEvent {
        let bytes = match BASE64.decode(response_b64) {
            Ok(b) => b,
            Err(_) => {
                self.state = State::Failed;
                return ResponderEvent::Failed {
                    reason: "decrypt_error".to_string(),
                };
            }
        };
        match responder.process_response(&bytes) {
            Ok(response) => {
                self.response = Some(response);
                self.state = State::Completed;
                ResponderEvent::ResponseReady
            }
            Err(e) => self.fail(&e),
        }
    }

    /// Navigation left the join screen. Absorbing.
    pub fn cancel(&mut self) -> ResponderEvent {
        self.state = State::Cancelled;
        ResponderEvent::None
    }

    /// Take the decrypted response once [`ResponderEvent::ResponseReady`]
    /// has been observed, for `Vauchi::adopt_device_link_response`.
    pub fn take_response(&mut self) -> Option<DeviceLinkResponse> {
        self.response.take()
    }

    /// Move to `Failed` and map the crypto error to a stable reason id
    /// (the join engine renders these via a `failure_detail` map, Slice 3).
    fn fail(&mut self, e: &ExchangeError) -> ResponderEvent {
        self.state = State::Failed;
        ResponderEvent::Failed {
            reason: failure_reason(e).to_string(),
        }
    }
}

/// Map an `ExchangeError` to a stable, user-agnostic failure id. Never
/// leaks the raw error text — the join engine maps these to sentences.
fn failure_reason(e: &ExchangeError) -> &'static str {
    match e {
        ExchangeError::TokenExpired
        | ExchangeError::QRExpired
        | ExchangeError::DeviceLinkQRExpired => "qr_expired",
        ExchangeError::InvalidSignature
        | ExchangeError::CryptoError
        | ExchangeError::SerializationFailed
        | ExchangeError::KeyAgreementFailed(_) => "decrypt_error",
        ExchangeError::InvalidQRFormat | ExchangeError::InvalidProtocolVersion => "invalid_qr",
        _ => "link_failed",
    }
}

// INLINE_TEST_REQUIRED: the machine is driven via the in-crate
// DeviceLinkBroker fakes and constructs the pub(crate) ClaimPayload —
// neither is reachable from the tests/ integration crate.
#[cfg(test)]
#[path = "device_link_responder_machine_tests.rs"]
mod tests;

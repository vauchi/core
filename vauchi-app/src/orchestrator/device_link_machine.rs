// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **initiator** state machine (slice 32l Phase 1).
//!
//! Replaces the spawned `vauchi-device-link-cycle` thread + bespoke
//! `DeviceLinkSessionListener` callback trait with a deterministic,
//! synchronous machine the engine owns and advances via the
//! `poll_notifications` tick — **one non-blocking relay step per
//! [`advance`]** through the [`DeviceLinkBroker`] seam (ADR-030: relay
//! is core's domain; not the hardware command/event protocol).
//!
//! Design:
//! `_private/docs/designs/2026-05-24-slice-32l-phase-1-device-link-state-machine-design.md`.
//!
//! State sequence (extracted from the old `run_initiator_cycle`):
//!
//! ```text
//! start ─exchange_offer→ AwaitingClaim {QR emitted}
//! AwaitingClaim ─advance: one exchange_complete─▶
//!       claimed       → AwaitingConfirmation
//!       now≥deadline  → Failed("qr_expired")
//!       else          → unchanged
//! AwaitingConfirmation ─confirm_manual / deny / timeout─▶ Finalizing | Failed
//! Finalizing ─advance: confirm_link + persist + send_response─▶ Completed | Failed
//! (any) ─cancel─▶ Cancelled (absorbing)
//! ```
//!
//! Time is passed explicitly as `now: u64` (matching the domain ops);
//! tests drive expiry by passing `now` values — no `Clock`/`Sleeper`,
//! no thread, no mpsc channel (CC-06).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use vauchi_core::exchange::{DeviceLinkInitiator, DeviceLinkRequest, ProximityProof};

use super::device_link_relay::DeviceLinkBroker;
use super::device_link_session::DeviceLinkPersistence;

/// Observable phase of the initiator machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitiatorPhase {
    AwaitingClaim,
    AwaitingConfirmation,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
}

/// What a transition produced. The engine maps each to a `ScreenModel`
/// via the existing `ui/app_engine/device_link.rs` handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitiatorEvent {
    /// No observable change this step (e.g. relay not yet claimed).
    None,
    /// QR is ready to display.
    QrReady {
        qr_data: String,
        expires_at_unix: u64,
    },
    /// Peer claimed; user must confirm or deny.
    ConfirmationRequired {
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        challenge: Vec<u8>,
    },
    /// Link completed; new device registered.
    Completed {
        device_name: String,
        device_index: u32,
    },
    /// Terminal failure (stable reason ids: `qr_expired`,
    /// `user_denied`, `user_confirm_timeout`, or a relay message).
    Failed { reason: String },
}

/// Internal state. `#[allow(dead_code)]`: the `AwaitingConfirmation` /
/// `Finalizing` variants and the carried domain data are constructed by
/// the transition logic landed in T1.2; the T1.1 skeleton only reaches
/// `AwaitingClaim` / `Failed` / `Cancelled`.
#[allow(dead_code)]
enum State {
    AwaitingClaim {
        broker_code: String,
    },
    AwaitingConfirmation {
        request: DeviceLinkRequest,
        sender_token: String,
    },
    Finalizing {
        request: DeviceLinkRequest,
        proof: ProximityProof,
        sender_token: String,
    },
    Completed,
    Failed,
    Cancelled,
}

/// Deterministic, poll-driven device-link initiator.
///
/// `#[allow(dead_code)]` on the fields the T1.2 transition logic reads
/// (`initiator`, `deadline_unix`, `confirm_deadline_unix`,
/// `persistence`) — the T1.1 skeleton only sets them.
#[allow(dead_code)]
pub struct DeviceLinkInitiatorMachine {
    initiator: DeviceLinkInitiator,
    state: State,
    deadline_unix: u64,
    confirm_deadline_unix: Option<u64>,
    persistence: Option<DeviceLinkPersistence>,
}

impl DeviceLinkInitiatorMachine {
    /// Create the relay offer and emit [`InitiatorEvent::QrReady`].
    /// `now` + `relay_timeout_secs` set the QR-expiry deadline
    /// (ADR-035 = 300 s in production).
    pub fn start(
        initiator: DeviceLinkInitiator,
        broker: &dyn DeviceLinkBroker,
        identity_id: &str,
        now: u64,
        relay_timeout_secs: u64,
        persistence: Option<DeviceLinkPersistence>,
    ) -> (Self, InitiatorEvent) {
        let (qr_data, expires_at_unix) = {
            let qr = initiator.qr();
            (qr.to_data_string(), qr.expires_at())
        };

        let (state, event) = match broker.exchange_offer(
            &BASE64.encode(identity_id.as_bytes()),
            Some(relay_timeout_secs),
        ) {
            Ok(broker_code) => (
                State::AwaitingClaim { broker_code },
                InitiatorEvent::QrReady {
                    qr_data,
                    expires_at_unix,
                },
            ),
            Err(e) => (
                State::Failed,
                InitiatorEvent::Failed {
                    reason: format!("relay offer failed: {e}"),
                },
            ),
        };

        (
            Self {
                initiator,
                state,
                deadline_unix: now.saturating_add(relay_timeout_secs),
                confirm_deadline_unix: None,
                persistence,
            },
            event,
        )
    }

    /// Current observable phase.
    pub fn phase(&self) -> InitiatorPhase {
        match &self.state {
            State::AwaitingClaim { .. } => InitiatorPhase::AwaitingClaim,
            State::AwaitingConfirmation { .. } => InitiatorPhase::AwaitingConfirmation,
            State::Finalizing { .. } => InitiatorPhase::Finalizing,
            State::Completed => InitiatorPhase::Completed,
            State::Failed => InitiatorPhase::Failed,
            State::Cancelled => InitiatorPhase::Cancelled,
        }
    }

    /// One non-blocking relay step. In `AwaitingClaim`: poll
    /// `exchange_complete` once → `ConfirmationRequired`, or
    /// `Failed("qr_expired")` once `now >= deadline`. In `Finalizing`:
    /// `confirm_link` + persist + `send_response` → `Completed`.
    ///
    /// T1.2: implement. The skeleton is a no-op so the relay/expiry/
    /// finalize tests stay RED.
    pub fn advance(&mut self, _broker: &dyn DeviceLinkBroker, _now: u64) -> InitiatorEvent {
        InitiatorEvent::None
    }

    /// User confirmed via matching codes (manual path). Builds the
    /// proximity proof and moves to `Finalizing`.
    ///
    /// T1.2: implement.
    pub fn confirm_manual(&mut self, _confirmation_code: String, _at: u64) -> InitiatorEvent {
        InitiatorEvent::None
    }

    /// User completed ultrasonic proximity verification.
    ///
    /// T1.2: implement.
    pub fn confirm_ultrasonic(&mut self, _challenge_response: Vec<u8>, _at: u64) -> InitiatorEvent {
        InitiatorEvent::None
    }

    /// User declined the link → `Failed("user_denied")`.
    ///
    /// T1.2: implement.
    pub fn deny(&mut self) -> InitiatorEvent {
        InitiatorEvent::None
    }

    /// Navigation left the device-linking screen. Absorbing.
    pub fn cancel(&mut self) -> InitiatorEvent {
        self.state = State::Cancelled;
        InitiatorEvent::None
    }
}

// INLINE_TEST_REQUIRED: state-machine unit tests drive the machine via
// the in-crate DeviceLinkBroker fake and decode pub(crate) ClaimPayload —
// neither is reachable from the tests/ integration crate.
#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::collections::VecDeque;

    use proptest::prelude::*;
    use vauchi_core::exchange::{DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder};
    use vauchi_core::identity::{DeviceRegistry, Identity};
    use vauchi_core::network::NetworkError;

    use crate::orchestrator::device_link_relay::ClaimPayload;

    const NOW: u64 = 1_700_000_000;
    const TIMEOUT: u64 = 300;

    fn create_test_registry(identity: &Identity) -> DeviceRegistry {
        let device_info = identity.device_info();
        let master_seed = [0x42u8; 32];
        DeviceRegistry::new(
            device_info.to_registered(&master_seed),
            identity.signing_keypair(),
        )
    }

    fn build_initiator(name: &str, seed: u8) -> DeviceLinkInitiator {
        let identity = Identity::create(name, 0);
        let registry = create_test_registry(&identity);
        DeviceLinkInitiator::new([seed; 32], &identity, registry, NOW)
    }

    /// Build the base64 claim payload a real responder would deposit,
    /// paired to `initiator`'s QR (real crypto — no mocking, ADR-002).
    fn responder_claim_b64(initiator: &DeviceLinkInitiator, response_code: &str) -> String {
        let scanned = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
        let mut responder =
            DeviceLinkResponder::from_qr(scanned, "New Phone".to_string(), NOW).unwrap();
        let encrypted_request = responder.create_request(NOW).unwrap();
        let claim = ClaimPayload {
            request: encrypted_request,
            response_code: response_code.to_string(),
        };
        BASE64.encode(serde_json::to_vec(&claim).unwrap())
    }

    /// Scriptable [`DeviceLinkBroker`] fake.
    struct FakeBroker {
        offer_code: String,
        complete_queue: RefCell<VecDeque<Result<Option<String>, NetworkError>>>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl FakeBroker {
        fn new(offer_code: &str) -> Self {
            Self {
                offer_code: offer_code.to_string(),
                complete_queue: RefCell::new(VecDeque::new()),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn push_complete(&self, r: Result<Option<String>, NetworkError>) {
            self.complete_queue.borrow_mut().push_back(r);
        }
    }

    impl DeviceLinkBroker for FakeBroker {
        fn exchange_offer(&self, _p: &str, _e: Option<u64>) -> Result<String, NetworkError> {
            self.calls.borrow_mut().push("offer");
            Ok(self.offer_code.clone())
        }
        fn exchange_claim(&self, _c: &str, _r: &str) -> Result<String, NetworkError> {
            self.calls.borrow_mut().push("claim");
            Ok(String::new())
        }
        fn exchange_complete(&self, _c: &str) -> Result<Option<String>, NetworkError> {
            self.calls.borrow_mut().push("complete");
            self.complete_queue
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(None))
        }
    }

    // ── Green: the R1 seam + start work end-to-end ─────────────────

    #[test]
    fn start_creates_offer_and_emits_qr_ready() {
        let initiator = build_initiator("Alice", 0x11);
        let expected_qr = initiator.qr().to_data_string();
        let expected_expiry = initiator.qr().expires_at();
        let broker = FakeBroker::new("BROKER123");

        let (m, event) =
            DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

        assert_eq!(m.phase(), InitiatorPhase::AwaitingClaim);
        assert_eq!(
            event,
            InitiatorEvent::QrReady {
                qr_data: expected_qr,
                expires_at_unix: expected_expiry,
            }
        );
        assert_eq!(*broker.calls.borrow(), vec!["offer"]);
    }

    #[test]
    fn cancel_is_absorbing() {
        let initiator = build_initiator("Alice", 0x11);
        let broker = FakeBroker::new("B");
        let (mut m, _) =
            DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

        assert_eq!(m.cancel(), InitiatorEvent::None);
        assert_eq!(m.phase(), InitiatorPhase::Cancelled);
        // Even a past-deadline advance must not resurrect the machine.
        let _ = m.advance(&broker, NOW + TIMEOUT + 10_000);
        assert_eq!(m.phase(), InitiatorPhase::Cancelled);
    }

    // ── RED until T1.2 implements the transitions ──────────────────

    #[test]
    fn advance_emits_qr_expired_at_deadline() {
        let initiator = build_initiator("Alice", 0x11);
        let broker = FakeBroker::new("B");
        broker.push_complete(Ok(None)); // not yet claimed
        let (mut m, _) =
            DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

        let event = m.advance(&broker, NOW + TIMEOUT + 1);
        assert_eq!(
            event,
            InitiatorEvent::Failed {
                reason: "qr_expired".to_string()
            }
        );
        assert_eq!(m.phase(), InitiatorPhase::Failed);
    }

    #[test]
    fn claim_then_confirm_manual_completes() {
        let initiator = build_initiator("Alice", 0x11);
        let claim_b64 = responder_claim_b64(&initiator, "RESP_CODE");
        let broker = FakeBroker::new("B");
        broker.push_complete(Ok(Some(claim_b64)));
        let (mut m, _) =
            DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

        let e1 = m.advance(&broker, NOW + 1);
        let code = match e1 {
            InitiatorEvent::ConfirmationRequired {
                confirmation_code, ..
            } => confirmation_code,
            other => panic!("expected ConfirmationRequired, got {other:?}"),
        };
        assert_eq!(m.phase(), InitiatorPhase::AwaitingConfirmation);

        let _ = m.confirm_manual(code, NOW + 2);
        let e3 = m.advance(&broker, NOW + 3);
        assert!(
            matches!(e3, InitiatorEvent::Completed { .. }),
            "expected Completed, got {e3:?}"
        );
        assert_eq!(m.phase(), InitiatorPhase::Completed);
    }

    #[test]
    fn deny_fails_with_user_denied() {
        let initiator = build_initiator("Alice", 0x11);
        let claim_b64 = responder_claim_b64(&initiator, "RESP_CODE");
        let broker = FakeBroker::new("B");
        broker.push_complete(Ok(Some(claim_b64)));
        let (mut m, _) =
            DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

        let _ = m.advance(&broker, NOW + 1);
        assert_eq!(m.phase(), InitiatorPhase::AwaitingConfirmation);
        let event = m.deny();
        assert_eq!(
            event,
            InitiatorEvent::Failed {
                reason: "user_denied".to_string()
            }
        );
        assert_eq!(m.phase(), InitiatorPhase::Failed);
    }

    // CC-13 stateful property: qr_expired fires exactly at the deadline
    // boundary while awaiting a claim, for any over-shoot.
    proptest! {
        #[test]
        fn qr_expired_at_deadline_boundary(overshoot in 1u64..100_000) {
            let initiator = build_initiator("Alice", 0x33);
            let broker = FakeBroker::new("B");
            broker.push_complete(Ok(None));
            broker.push_complete(Ok(None));
            let (mut m, _) =
                DeviceLinkInitiatorMachine::start(initiator, &broker, "id", NOW, TIMEOUT, None);

            // Just before the deadline: still awaiting.
            let before = m.advance(&broker, NOW + TIMEOUT - 1);
            prop_assert_eq!(before, InitiatorEvent::None);
            prop_assert_eq!(m.phase(), InitiatorPhase::AwaitingClaim);

            // At/after the deadline: qr_expired, terminal.
            let after = m.advance(&broker, NOW + TIMEOUT + overshoot);
            prop_assert_eq!(after, InitiatorEvent::Failed { reason: "qr_expired".to_string() });
            prop_assert_eq!(m.phase(), InitiatorPhase::Failed);
        }
    }
}

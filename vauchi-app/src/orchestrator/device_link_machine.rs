// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **initiator** state machine (slice 32l Phase 1).
//!
//! Replaces the spawned `vauchi-device-link-cycle` thread + bespoke
//! `DeviceLinkSessionListener` callback trait with a deterministic,
//! synchronous machine the engine owns and advances via the
//! `poll_notifications` tick — **one non-blocking relay step per
//! [`DeviceLinkInitiatorMachine::advance`]** through the
//! [`DeviceLinkBroker`] seam (ADR-030: relay is core's domain; not the
//! hardware command/event protocol).
//!
//! Design:
//! `_private/docs/designs/2026-05-24-slice-32l-phase-1-device-link-state-machine-design.md`.
//!
//! State sequence (extracted from the old `run_initiator_cycle`):
//!
//! ```text
//! new ─(no I/O)→ CreatingOffer
//! CreatingOffer ─advance: exchange_offer─▶ AwaitingClaim {QR emitted} | Failed
//! AwaitingClaim ─advance: one exchange_complete─▶
//!       claimed       → AwaitingConfirmation
//!       now≥deadline  → Failed("qr_expired")
//!       else          → unchanged
//! AwaitingConfirmation ─confirm_manual / deny / timeout─▶ Finalizing | Failed
//! Finalizing ─advance: confirm_link + persist + send_response─▶ Completed | Failed
//! (any) ─cancel─▶ Cancelled (absorbing)
//! ```
//!
//! **All relay I/O happens inside `advance`** (driven from the poll
//! thread) — `new` touches nothing, so navigation into the screen
//! never blocks the action thread on a network round-trip. Time is
//! passed explicitly as `now: u64`; tests drive expiry by passing
//! `now` values — no `Clock`/`Sleeper`, no thread, no mpsc channel
//! (CC-06).

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use vauchi_core::api::DeviceSyncOrchestrator;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkJoinInvitation, DeviceLinkRequest, ProximityProof,
};
use vauchi_core::identity::Identity;
use vauchi_core::storage::Storage;

use super::device_link_relay::{ClaimPayload, DeviceLinkBroker};

/// Persistence handle the machine uses to save the updated device
/// registry on a successful link.
#[derive(Clone)]
pub struct DeviceLinkPersistence {
    pub storage_path: PathBuf,
    pub storage_key: SymmetricKey,
}

/// User-confirmation window once the peer has claimed the offer.
const USER_CONFIRM_TIMEOUT_S: u64 = 60;

/// Observable phase of the initiator machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitiatorPhase {
    /// Constructed, offer not yet posted (no relay I/O has occurred).
    Preparing,
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

/// Internal state. Carries the owned domain data each phase needs.
enum State {
    CreatingOffer,
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
pub struct DeviceLinkInitiatorMachine {
    initiator: DeviceLinkInitiator,
    state: State,
    identity_id: String,
    relay_timeout_secs: u64,
    deadline_unix: u64,
    confirm_deadline_unix: Option<u64>,
    persistence: Option<DeviceLinkPersistence>,
}

impl DeviceLinkInitiatorMachine {
    /// Construct the machine in `Preparing` — **no relay I/O**. The
    /// offer is posted on the first [`advance`](Self::advance), so the
    /// caller (navigation/action thread) never blocks on the network.
    pub fn new(
        initiator: DeviceLinkInitiator,
        identity_id: String,
        relay_timeout_secs: u64,
        persistence: Option<DeviceLinkPersistence>,
    ) -> Self {
        Self {
            initiator,
            state: State::CreatingOffer,
            identity_id,
            relay_timeout_secs,
            deadline_unix: 0,
            confirm_deadline_unix: None,
            persistence,
        }
    }

    /// Current observable phase.
    pub fn phase(&self) -> InitiatorPhase {
        match &self.state {
            State::CreatingOffer => InitiatorPhase::Preparing,
            State::AwaitingClaim { .. } => InitiatorPhase::AwaitingClaim,
            State::AwaitingConfirmation { .. } => InitiatorPhase::AwaitingConfirmation,
            State::Finalizing { .. } => InitiatorPhase::Finalizing,
            State::Completed => InitiatorPhase::Completed,
            State::Failed => InitiatorPhase::Failed,
            State::Cancelled => InitiatorPhase::Cancelled,
        }
    }

    /// Build a [`DeviceLinkJoinInvitation`] for the current session.
    ///
    /// Available once the offer has been posted and the relay returned the
    /// rendezvous code (i.e., state is `AwaitingClaim`). Before that the
    /// broker code is not yet known. The invitation URL is what a joining
    /// device scans or receives to start its responder machine.
    pub fn join_invitation(&self) -> Option<DeviceLinkJoinInvitation> {
        let broker_code = match &self.state {
            State::AwaitingClaim { broker_code } => broker_code.clone(),
            _ => return None,
        };
        Some(DeviceLinkJoinInvitation {
            qr_data: self.initiator.qr().to_data_string(),
            broker_code,
            relay_url: None,
        })
    }

    /// One non-blocking relay step. In `CreatingOffer`: post the offer
    /// → `QrReady`. In `AwaitingClaim`: poll `exchange_complete` once →
    /// `ConfirmationRequired`, or `Failed("qr_expired")` once
    /// `now >= deadline`. In `AwaitingConfirmation`: surfaces
    /// `user_confirm_timeout`. In `Finalizing`: `confirm_link` +
    /// persist + `send_response` → `Completed`.
    pub fn advance(&mut self, broker: &dyn DeviceLinkBroker, now: u64) -> InitiatorEvent {
        // `State::Failed` is a transient placeholder; every arm
        // reassigns `self.state` before returning.
        match std::mem::replace(&mut self.state, State::Failed) {
            State::CreatingOffer => self.create_offer(broker, now),
            State::AwaitingClaim { broker_code } => {
                if now >= self.deadline_unix {
                    self.state = State::Failed;
                    return InitiatorEvent::Failed {
                        reason: "qr_expired".to_string(),
                    };
                }
                match broker.exchange_complete(&broker_code) {
                    Ok(Some(claim_b64)) => self.on_claim(&claim_b64, now),
                    Ok(None) => {
                        self.state = State::AwaitingClaim { broker_code };
                        InitiatorEvent::None
                    }
                    Err(e) => {
                        self.state = State::Failed;
                        InitiatorEvent::Failed {
                            reason: format!("relay poll failed: {e}"),
                        }
                    }
                }
            }
            State::AwaitingConfirmation {
                request,
                sender_token,
            } => {
                if self.confirm_deadline_unix.is_some_and(|d| now >= d) {
                    self.state = State::Failed;
                    return InitiatorEvent::Failed {
                        reason: "user_confirm_timeout".to_string(),
                    };
                }
                self.state = State::AwaitingConfirmation {
                    request,
                    sender_token,
                };
                InitiatorEvent::None
            }
            State::Finalizing {
                request,
                proof,
                sender_token,
            } => self.finalize(broker, &request, &proof, &sender_token, now),
            terminal => {
                self.state = terminal;
                InitiatorEvent::None
            }
        }
    }

    /// User confirmed via matching codes (manual path). Builds the
    /// proximity proof and moves to `Finalizing`; the relay send
    /// happens on the next [`advance`](Self::advance).
    pub fn confirm_manual(&mut self, confirmation_code: String, at: u64) -> InitiatorEvent {
        match std::mem::replace(&mut self.state, State::Failed) {
            State::AwaitingConfirmation {
                request,
                sender_token,
            } => {
                let proof = ProximityProof::manual_confirmation(
                    self.initiator.qr().link_key(),
                    &confirmation_code,
                    at,
                );
                self.state = State::Finalizing {
                    request,
                    proof,
                    sender_token,
                };
                InitiatorEvent::None
            }
            other => {
                self.state = other;
                InitiatorEvent::None
            }
        }
    }

    /// User completed ultrasonic proximity verification.
    pub fn confirm_ultrasonic(&mut self, challenge_response: Vec<u8>, at: u64) -> InitiatorEvent {
        match std::mem::replace(&mut self.state, State::Failed) {
            State::AwaitingConfirmation {
                request,
                sender_token,
            } => match <[u8; 16]>::try_from(challenge_response.as_slice()) {
                Ok(bytes) => {
                    self.state = State::Finalizing {
                        request,
                        proof: ProximityProof::Ultrasonic {
                            challenge_response: bytes,
                            verified_at: at,
                        },
                        sender_token,
                    };
                    InitiatorEvent::None
                }
                Err(_) => {
                    self.state = State::Failed;
                    InitiatorEvent::Failed {
                        reason: "challenge_response must be exactly 16 bytes".to_string(),
                    }
                }
            },
            other => {
                self.state = other;
                InitiatorEvent::None
            }
        }
    }

    /// User declined the link → `Failed("user_denied")`.
    pub fn deny(&mut self) -> InitiatorEvent {
        if matches!(self.state, State::AwaitingConfirmation { .. }) {
            self.state = State::Failed;
            InitiatorEvent::Failed {
                reason: "user_denied".to_string(),
            }
        } else {
            InitiatorEvent::None
        }
    }

    /// Navigation left the device-linking screen. Absorbing.
    pub fn cancel(&mut self) -> InitiatorEvent {
        self.state = State::Cancelled;
        InitiatorEvent::None
    }

    /// Post the relay offer and emit `QrReady`. Sets the QR-expiry
    /// deadline from `now` (ADR-035 budget = `relay_timeout_secs`).
    fn create_offer(&mut self, broker: &dyn DeviceLinkBroker, now: u64) -> InitiatorEvent {
        let (qr_data, expires_at_unix) = {
            let qr = self.initiator.qr();
            (qr.to_data_string(), qr.expires_at())
        };
        match broker.exchange_offer(
            &BASE64.encode(self.identity_id.as_bytes()),
            Some(self.relay_timeout_secs),
        ) {
            Ok(broker_code) => {
                self.deadline_unix = now.saturating_add(self.relay_timeout_secs);
                self.state = State::AwaitingClaim { broker_code };
                InitiatorEvent::QrReady {
                    qr_data,
                    expires_at_unix,
                }
            }
            Err(e) => {
                self.state = State::Failed;
                InitiatorEvent::Failed {
                    reason: format!("relay offer failed: {e}"),
                }
            }
        }
    }

    /// Decode a claim, prepare the confirmation, and move to
    /// `AwaitingConfirmation`. Self-contained so `advance` stays flat.
    fn on_claim(&mut self, claim_b64: &str, now: u64) -> InitiatorEvent {
        let (request_bytes, sender_token) = match decode_claim(claim_b64) {
            Ok(pair) => pair,
            Err(reason) => {
                self.state = State::Failed;
                return InitiatorEvent::Failed { reason };
            }
        };
        match self.initiator.prepare_confirmation(&request_bytes) {
            Ok((confirmation, request)) => {
                let challenge = self.initiator.proximity_challenge().to_vec();
                self.confirm_deadline_unix = Some(now.saturating_add(USER_CONFIRM_TIMEOUT_S));
                self.state = State::AwaitingConfirmation {
                    request,
                    sender_token,
                };
                InitiatorEvent::ConfirmationRequired {
                    device_name: confirmation.device_name,
                    confirmation_code: confirmation.confirmation_code,
                    identity_fingerprint: confirmation.identity_fingerprint,
                    challenge,
                }
            }
            Err(e) => {
                self.state = State::Failed;
                InitiatorEvent::Failed {
                    reason: format!("prepare_confirmation: {e}"),
                }
            }
        }
    }

    /// `confirm_link` + persist registry + `send_response` over the
    /// relay → `Completed`.
    fn finalize(
        &mut self,
        broker: &dyn DeviceLinkBroker,
        request: &DeviceLinkRequest,
        proof: &ProximityProof,
        sender_token: &str,
        now: u64,
    ) -> InitiatorEvent {
        let (encrypted_response, registry, device_info) =
            match self.initiator.confirm_link(request, proof, now) {
                Ok(triple) => triple,
                Err(e) => {
                    self.state = State::Failed;
                    return InitiatorEvent::Failed {
                        reason: format!("confirm_link: {e}"),
                    };
                }
            };

        if let Some(ctx) = &self.persistence {
            let save = (|| -> Result<(), String> {
                let storage = Storage::open(&ctx.storage_path, ctx.storage_key.clone())
                    .map_err(|error| error.to_string())?;
                let (identity_bytes, _) = storage
                    .identity()
                    .load_identity()
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "identity missing while finalizing device link".to_string())?;
                let identity = Identity::from_storage_bytes(&identity_bytes, now)
                    .map_err(|error| error.to_string())?;
                DeviceSyncOrchestrator::persist_device_registry_change(
                    &storage, &identity, &registry, now,
                )
                .map_err(|error| error.to_string())
            })();
            if let Err(e) = save {
                self.state = State::Failed;
                return InitiatorEvent::Failed {
                    reason: format!("save_device_registry: {e}"),
                };
            }
        }

        match broker.exchange_claim(sender_token, &BASE64.encode(&encrypted_response)) {
            Ok(_) => {
                let event = InitiatorEvent::Completed {
                    device_name: device_info.device_name().to_string(),
                    device_index: device_info.device_index(),
                };
                self.state = State::Completed;
                event
            }
            Err(e) => {
                self.state = State::Failed;
                InitiatorEvent::Failed {
                    reason: format!("send_response: {e}"),
                }
            }
        }
    }
}

/// Decode the base64 `ClaimPayload` a responder deposited.
fn decode_claim(claim_b64: &str) -> Result<(Vec<u8>, String), String> {
    let bytes = BASE64
        .decode(claim_b64)
        .map_err(|e| format!("claim decode: {e}"))?;
    let claim: ClaimPayload =
        serde_json::from_slice(&bytes).map_err(|e| format!("claim parse: {e}"))?;
    Ok((claim.request, claim.response_code))
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

    fn machine(initiator: DeviceLinkInitiator) -> DeviceLinkInitiatorMachine {
        DeviceLinkInitiatorMachine::new(initiator, "id".to_string(), TIMEOUT, None)
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

    // ── new() does no I/O; the first advance posts the offer ───────

    // @internal
    #[test]
    fn new_starts_in_preparing_without_touching_the_relay() {
        let broker = FakeBroker::new("B");
        let m = machine(build_initiator("Alice", 0x11));
        assert_eq!(m.phase(), InitiatorPhase::Preparing);
        assert!(
            broker.calls.borrow().is_empty(),
            "new() must not call the relay"
        );
    }

    // @internal
    #[test]
    fn first_advance_creates_offer_and_emits_qr_ready() {
        let initiator = build_initiator("Alice", 0x11);
        let expected_qr = initiator.qr().to_data_string();
        let expected_expiry = initiator.qr().expires_at();
        let broker = FakeBroker::new("BROKER123");
        let mut m = machine(initiator);

        let event = m.advance(&broker, NOW);

        assert_eq!(
            event,
            InitiatorEvent::QrReady {
                qr_data: expected_qr,
                expires_at_unix: expected_expiry,
            }
        );
        assert_eq!(m.phase(), InitiatorPhase::AwaitingClaim);
        assert_eq!(*broker.calls.borrow(), vec!["offer"]);
    }

    // @internal
    #[test]
    fn join_invitation_available_after_first_advance() {
        let initiator = build_initiator("Alice", 0x11);
        let expected_qr = initiator.qr().to_data_string();
        let broker = FakeBroker::new("BROKER123");
        let mut m = machine(initiator);

        assert!(
            m.join_invitation().is_none(),
            "invitation unavailable before offer"
        );
        let _ = m.advance(&broker, NOW);

        let invitation = m
            .join_invitation()
            .expect("invitation available after offer is posted");
        assert_eq!(invitation.qr_data, expected_qr);
        assert_eq!(invitation.broker_code, "BROKER123");
        assert!(invitation.relay_url.is_none());

        let parsed_url = DeviceLinkJoinInvitation::parse_url(&invitation.to_url())
            .expect("round-trips through URL");
        assert_eq!(parsed_url, invitation);
    }

    // @internal
    #[test]
    fn cancel_is_absorbing() {
        let broker = FakeBroker::new("B");
        let mut m = machine(build_initiator("Alice", 0x11));

        assert_eq!(m.cancel(), InitiatorEvent::None);
        assert_eq!(m.phase(), InitiatorPhase::Cancelled);
        // Even a past-deadline advance must not resurrect the machine.
        let _ = m.advance(&broker, NOW + TIMEOUT + 10_000);
        assert_eq!(m.phase(), InitiatorPhase::Cancelled);
    }

    // ── Relay / confirm / deny transitions ─────────────────────────

    // @internal
    #[test]
    fn advance_emits_qr_expired_at_deadline() {
        let broker = FakeBroker::new("B");
        let mut m = machine(build_initiator("Alice", 0x11));

        let _ = m.advance(&broker, NOW); // offer → deadline = NOW + TIMEOUT
        assert_eq!(m.phase(), InitiatorPhase::AwaitingClaim);
        let event = m.advance(&broker, NOW + TIMEOUT + 1);
        assert_eq!(
            event,
            InitiatorEvent::Failed {
                reason: "qr_expired".to_string()
            }
        );
        assert_eq!(m.phase(), InitiatorPhase::Failed);
    }

    // @internal
    #[test]
    fn claim_then_confirm_manual_completes() {
        let initiator = build_initiator("Alice", 0x11);
        let claim_b64 = responder_claim_b64(&initiator, "RESP_CODE");
        let broker = FakeBroker::new("B");
        broker.push_complete(Ok(Some(claim_b64)));
        let mut m = machine(initiator);

        let _ = m.advance(&broker, NOW); // offer
        let e1 = m.advance(&broker, NOW + 1); // poll → claim
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

    /// The headline claim of ADR-070 Phase 1: a whole ceremony runs with
    /// no relay in it. Uses the real rendezvous rather than `FakeBroker`,
    /// so the claim genuinely travels initiator → rendezvous → initiator,
    /// and real crypto throughout (ADR-002 — nothing mocked).
    ///
    /// If the local broker stopped wiring through, the claim would never
    /// reach the machine and the second advance would not be
    /// `ConfirmationRequired`.
    // @internal
    #[test]
    fn a_whole_ceremony_runs_against_a_local_broker_with_no_relay() {
        use crate::orchestrator::local_rendezvous::{
            LocalDeviceLinkBroker, SingleCeremonyRendezvous,
        };
        use std::sync::Arc;

        let initiator = build_initiator("Alice", 0x11);
        let claim_b64 = responder_claim_b64(&initiator, "RESP_CODE");
        let rendezvous = Arc::new(SingleCeremonyRendezvous::new());
        let broker = LocalDeviceLinkBroker::new("LOCAL".to_string(), Arc::clone(&rendezvous));
        let mut m = machine(initiator);

        // The offer lands in the local rendezvous; no relay is reachable.
        let e0 = m.advance(&broker, NOW);
        assert!(
            matches!(e0, InitiatorEvent::QrReady { .. }),
            "expected QrReady, got {e0:?}"
        );

        // The joiner opens its response channel, whose code it embeds in
        // the claim below. The initiator claims *that* in `Finalizing` to
        // deliver its response — the second leg of the ceremony.
        rendezvous
            .offer("RESP_CODE".to_string(), "b64-joiner-channel".to_string())
            .expect("joiner opens its response channel");

        // The joiner deposits its claim through the same rendezvous — the
        // step the relay would otherwise have brokered.
        rendezvous
            .claim("LOCAL", &claim_b64)
            .expect("joiner claims the ceremony");

        let e1 = m.advance(&broker, NOW + 1);
        let code = match e1 {
            InitiatorEvent::ConfirmationRequired {
                confirmation_code, ..
            } => confirmation_code,
            other => panic!("expected ConfirmationRequired, got {other:?}"),
        };

        let _ = m.confirm_manual(code, NOW + 2);
        let e3 = m.advance(&broker, NOW + 3);
        assert!(
            matches!(e3, InitiatorEvent::Completed { .. }),
            "expected Completed, got {e3:?}"
        );
        assert_eq!(m.phase(), InitiatorPhase::Completed);
    }

    // @internal
    #[test]
    fn deny_fails_with_user_denied() {
        let initiator = build_initiator("Alice", 0x11);
        let claim_b64 = responder_claim_b64(&initiator, "RESP_CODE");
        let broker = FakeBroker::new("B");
        broker.push_complete(Ok(Some(claim_b64)));
        let mut m = machine(initiator);

        let _ = m.advance(&broker, NOW); // offer
        let _ = m.advance(&broker, NOW + 1); // poll → claim
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
        // @internal
        #[test]
        fn qr_expired_at_deadline_boundary(overshoot in 1u64..100_000) {
            let broker = FakeBroker::new("B");
            broker.push_complete(Ok(None));
            let mut m = machine(build_initiator("Alice", 0x33));

            let _ = m.advance(&broker, NOW); // offer → deadline = NOW + TIMEOUT

            // Just before the deadline: poll returns None, still awaiting.
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

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use base64::Engine;
use std::cell::RefCell;
use std::collections::VecDeque;

use vauchi_core::exchange::{DeviceLinkInitiator, DeviceLinkJoinInvitation, ProximityProof};
use vauchi_core::identity::{DeviceRegistry, Identity};
use vauchi_core::network::NetworkError;

use super::{
    DeviceLinkBroker, DeviceLinkResponderMachine, ResponderEvent, ResponderMachineError,
    ResponderPhase,
};

const NOW: u64 = 1_700_000_000;
const TIMEOUT: u64 = 300;
const BROKER_CODE: &str = "BROKER123";

fn create_test_registry(identity: &Identity) -> DeviceRegistry {
    let master_seed = [0x42u8; 32];
    DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    )
}

fn build_initiator(name: &str, seed: u8) -> DeviceLinkInitiator {
    let identity = Identity::create(name, 0);
    let registry = create_test_registry(&identity);
    DeviceLinkInitiator::new([seed; 32], &identity, registry, NOW)
}

fn build_invitation(initiator: &DeviceLinkInitiator) -> DeviceLinkJoinInvitation {
    DeviceLinkJoinInvitation {
        qr_data: initiator.qr().to_data_string(),
        broker_code: BROKER_CODE.to_string(),
        relay_url: None,
    }
}

/// Scriptable [`DeviceLinkBroker`] fake.
struct FakeBroker {
    complete_queue: RefCell<VecDeque<Result<Option<String>, NetworkError>>>,
    claims: RefCell<Vec<(String, String)>>,
}

impl FakeBroker {
    fn new() -> Self {
        Self {
            complete_queue: RefCell::new(VecDeque::new()),
            claims: RefCell::new(Vec::new()),
        }
    }
    fn push_complete(&self, r: Result<Option<String>, NetworkError>) {
        self.complete_queue.borrow_mut().push_back(r);
    }
}

impl DeviceLinkBroker for FakeBroker {
    fn exchange_offer(
        &self,
        _payload_b64: &str,
        _expires_secs: Option<u64>,
    ) -> Result<String, NetworkError> {
        Ok(BROKER_CODE.to_string())
    }
    fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
        self.claims
            .borrow_mut()
            .push((code.to_string(), response_b64.to_string()));
        Ok(String::new())
    }
    fn exchange_complete(&self, _code: &str) -> Result<Option<String>, NetworkError> {
        self.complete_queue
            .borrow_mut()
            .pop_front()
            .unwrap_or(Ok(None))
    }
}

// @internal
/// The two devices must stop waiting at the same moment. The host's
/// window opens when it mints the QR; the joiner's opened when it
/// *scanned* one, so a QR that sat on screen for a while left the joiner
/// outliving the host — observed on hardware as the host showing "QR code
/// expired" while the joiner still said "Waiting for approval…"
/// (2026-08-14-device-link-host-never-shows-the-confirmation-code).
///
/// `DeviceLinkQR` carries an absolute `expires_at`, so the ceremony has
/// one deadline rather than two clocks started at different times.
// @scenario: device_management :: a late joiner expires with the QR, not 300s later
#[test]
fn the_joiner_deadline_follows_the_qr_not_the_moment_it_was_scanned() {
    let initiator = build_initiator("Alice", 0x11);
    let qr_expires_at = initiator.qr().expires_at();
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();

    // Scanned late in the QR's own window.
    let scanned_at = qr_expires_at - 50;
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();
    assert!(
        matches!(
            m.advance(&broker, scanned_at),
            ResponderEvent::RequestPosted { .. }
        ),
        "a QR inside its window must still be joinable"
    );

    // One second past the QR's expiry — the host has already given up.
    broker.push_complete(Ok(None));
    let event = m.advance(&broker, qr_expires_at + 1);
    assert!(
        matches!(event, ResponderEvent::Failed { ref reason } if reason == "qr_expired"),
        "the joiner must expire with the QR rather than run its own \
         window from the scan, got {event:?}"
    );
}

#[test]
fn new_parses_invitation_without_relay_io() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);

    let broker = FakeBroker::new();
    let m = DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    assert_eq!(m.phase(), ResponderPhase::Scanning);
    assert!(
        broker.claims.borrow().is_empty(),
        "new() must not call the relay"
    );
}

// @internal
#[test]
fn new_fails_on_invalid_qr_data() {
    let invitation = DeviceLinkJoinInvitation {
        qr_data: "not-a-valid-qr".to_string(),
        broker_code: BROKER_CODE.to_string(),
        relay_url: None,
    };

    let result = DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT);
    assert!(
        matches!(result, Err(ResponderMachineError::InvalidQr(_))),
        "expected QR parse failure"
    );
}

// @internal
#[test]
fn first_advance_posts_request_and_emits_confirmation_code() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    let event = m.advance(&broker, NOW);

    assert!(
        matches!(event, ResponderEvent::RequestPosted { .. }),
        "expected RequestPosted, got {event:?}"
    );
    assert_eq!(m.phase(), ResponderPhase::AwaitingResponse);
    assert_eq!(broker.claims.borrow().len(), 1);
    assert_eq!(broker.claims.borrow()[0].0, BROKER_CODE);
}

// @internal
#[test]
fn cancel_is_absorbing() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    assert_eq!(m.cancel(), ResponderEvent::None);
    assert_eq!(m.phase(), ResponderPhase::Cancelled);
    // Even a past-deadline advance must not resurrect the machine.
    let _ = m.advance(&broker, NOW + TIMEOUT + 10_000);
    assert_eq!(m.phase(), ResponderPhase::Cancelled);
}

// @internal
#[test]
fn advance_emits_qr_expired_at_deadline() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    let _ = m.advance(&broker, NOW); // post request → deadline = NOW + TIMEOUT
    assert_eq!(m.phase(), ResponderPhase::AwaitingResponse);
    let event = m.advance(&broker, NOW + TIMEOUT + 1);
    assert_eq!(
        event,
        ResponderEvent::Failed {
            reason: "qr_expired".to_string()
        }
    );
    assert_eq!(m.phase(), ResponderPhase::Failed);
}

// @internal
#[test]
fn advance_polls_until_response_ready() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();
    broker.push_complete(Ok(None));
    broker.push_complete(Ok(None));
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    let _ = m.advance(&broker, NOW); // post request
    assert_eq!(m.phase(), ResponderPhase::AwaitingResponse);

    let first = m.advance(&broker, NOW + 1);
    assert_eq!(first, ResponderEvent::None);
    assert_eq!(m.phase(), ResponderPhase::AwaitingResponse);

    let second = m.advance(&broker, NOW + 2);
    assert_eq!(second, ResponderEvent::None);
    assert_eq!(m.phase(), ResponderPhase::AwaitingResponse);
}

// @internal
#[test]
fn full_join_completes() {
    let initiator = build_initiator("Alice", 0x11);
    let invitation = build_invitation(&initiator);
    let broker = FakeBroker::new();
    let mut m =
        DeviceLinkResponderMachine::new(invitation, "My Phone".to_string(), TIMEOUT).unwrap();

    // Responder posts its encrypted request.
    let request_posted = m.advance(&broker, NOW);
    let confirmation_code = match request_posted {
        ResponderEvent::RequestPosted { confirmation_code } => confirmation_code,
        other => panic!("expected RequestPosted, got {other:?}"),
    };

    // Recover the encrypted request the responder deposited on the broker.
    let claim_b64 = &broker.claims.borrow()[0].1;
    let claim_bytes = base64::engine::general_purpose::STANDARD
        .decode(claim_b64)
        .unwrap();
    let claim: super::ClaimPayload = serde_json::from_slice(&claim_bytes).unwrap();

    // Existing device prepares confirmation and confirms using the same code.
    let (confirmation, request) = initiator.prepare_confirmation(&claim.request).unwrap();
    assert_eq!(confirmation.confirmation_code, confirmation_code);

    let proof = ProximityProof::Ultrasonic {
        challenge_response: initiator.proximity_challenge(),
        verified_at: NOW,
    };
    let (encrypted_response, _registry, _new_device) =
        initiator.confirm_link(&request, &proof, NOW).unwrap();

    // Relay delivers the encrypted response to the responder's return channel.
    broker.push_complete(Ok(Some(
        base64::engine::general_purpose::STANDARD.encode(&encrypted_response),
    )));

    let event = m.advance(&broker, NOW + 1);
    assert_eq!(event, ResponderEvent::ResponseReady);
    assert_eq!(m.phase(), ResponderPhase::Completed);

    let response = m
        .take_response()
        .expect("response present after ResponseReady");
    assert_eq!(response.display_name(), "Alice");
    assert_eq!(response.device_index(), 1);
}

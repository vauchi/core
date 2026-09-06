// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Both device-link machines, real, against one broker.
//!
//! Everything else drives **one** machine and simulates its peer: the
//! `FakeBroker`s return a fixed code and an empty claim,
//! `a_whole_ceremony_runs_against_a_local_broker_with_no_relay` hand-mints
//! the joiner's claim and offers a literal `"b64-joiner-channel"`, and
//! `device_link_join_adopt_tests` runs the crypto dance directly without
//! either machine. So no test has ever observed the two machines agreeing
//! with each other, and on hardware a completed link has never been
//! observed either — the Maestro flow asserts only that QR generation
//! finishes (`2026-08-14-two-device-linking-has-no-automatable-path`).
//!
//! The property that matters to a user is the confirmation code: both
//! devices show one, the human compares them, and a mismatch means abort.
//! Nothing asserted they agree until here.

#![cfg(feature = "network-http")]

use std::collections::HashMap;
use std::sync::Mutex;

use vauchi_app::orchestrator::device_link_machine::{
    DeviceLinkInitiatorMachine, InitiatorEvent, InitiatorPhase,
};
use vauchi_app::orchestrator::device_link_relay::DeviceLinkBroker;
use vauchi_app::orchestrator::device_link_responder_machine::{
    DeviceLinkResponderMachine, ResponderEvent,
};
use vauchi_core::exchange::DeviceLinkInitiator;
use vauchi_core::identity::{DeviceRegistry, Identity};
use vauchi_core::network::NetworkError;

const NOW: u64 = 1_800_000_000;
const TIMEOUT_SECS: u64 = 300;

/// A broker with the relay's semantics rather than a stub's.
///
/// Two rules matter and both are enforced here, because production hit
/// each of them: the relay **mints** every code
/// (`relay/src/exchange_broker.rs`), and it rejects any code that is not
/// exactly six digits (`relay/src/http_api.rs:1250`). A broker that
/// accepts whatever string it is handed cannot observe a peer inventing
/// its own channel name, which is what shipped.
#[derive(Default)]
struct RelayLikeBroker {
    slots: Mutex<HashMap<String, Slot>>,
    next_code: Mutex<u32>,
}

#[derive(Default)]
struct Slot {
    offered: String,
    response: Option<String>,
}

impl RelayLikeBroker {
    fn new() -> Self {
        Self::default()
    }
}

impl DeviceLinkBroker for RelayLikeBroker {
    fn exchange_offer(
        &self,
        payload_b64: &str,
        _expires_secs: Option<u64>,
    ) -> Result<String, NetworkError> {
        let mut n = self.next_code.lock().expect("uncontended");
        *n += 1;
        let code = format!("{:06}", 100_000 + *n);
        self.slots.lock().expect("uncontended").insert(
            code.clone(),
            Slot {
                offered: payload_b64.to_string(),
                response: None,
            },
        );
        Ok(code)
    }

    fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
        // Mirrors the relay's own guard, verbatim in effect.
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(NetworkError::InvalidMessage(
                "exchange_claim failed: code must be exactly 6 digits".into(),
            ));
        }
        let mut slots = self.slots.lock().expect("uncontended");
        // The relay creates the slot on first claim, so a peer may claim
        // a code before anything was offered under it.
        let slot = slots.entry(code.to_string()).or_default();
        if slot.response.is_some() {
            return Err(NetworkError::RelayRejected("already claimed".into()));
        }
        slot.response = Some(response_b64.to_string());
        Ok(slot.offered.clone())
    }

    fn exchange_complete(&self, code: &str) -> Result<Option<String>, NetworkError> {
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(NetworkError::InvalidMessage(
                "exchange_complete failed: code must be exactly 6 digits".into(),
            ));
        }
        Ok(self
            .slots
            .lock()
            .expect("uncontended")
            .get(code)
            .and_then(|s| s.response.clone()))
    }
}

/// A real initiator for an identity that owns exactly one device.
fn initiator_for(display_name: &str, master_seed: [u8; 32]) -> (DeviceLinkInitiator, Identity) {
    let identity = Identity::create(display_name, NOW);
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    (
        DeviceLinkInitiator::new(master_seed, &identity, registry, NOW),
        identity,
    )
}

// @scenario: device_management :: two devices complete a link and agree on the code
#[test]
fn both_machines_complete_a_link_and_show_the_same_confirmation_code() {
    let master_seed = [0x5Au8; 32];
    let (initiator, _identity) = initiator_for("Alice", master_seed);
    let broker = RelayLikeBroker::new();

    let mut host = DeviceLinkInitiatorMachine::new(
        initiator,
        "alice-identity".to_string(),
        TIMEOUT_SECS,
        None,
    );

    // 1. The host publishes its offer and renders the QR.
    let first = host.advance(&broker, NOW);
    assert!(
        matches!(first, InitiatorEvent::QrReady { .. }),
        "the host must publish an offer before a joiner can find it, got {first:?}"
    );
    let invitation = host
        .join_invitation()
        .expect("a QR-ready host must expose the invitation the joiner scans");

    // 2. The joiner scans it and posts its request.
    let mut joiner =
        DeviceLinkResponderMachine::new(invitation, "New Phone".to_string(), TIMEOUT_SECS)
            .expect("a freshly minted invitation must parse");

    let joiner_code = match joiner.advance(&broker, NOW + 1) {
        ResponderEvent::RequestPosted { confirmation_code } => confirmation_code,
        other => panic!("expected RequestPosted, got {other:?}"),
    };

    // 3. The host picks the request up and shows its own code.
    let host_code = match host.advance(&broker, NOW + 2) {
        InitiatorEvent::ConfirmationRequired {
            confirmation_code, ..
        } => confirmation_code,
        other => panic!("expected ConfirmationRequired, got {other:?}"),
    };

    // The whole point of the ceremony: the human compares two screens.
    assert_eq!(
        host_code, joiner_code,
        "both devices must show the same confirmation code — if they can \
         disagree, comparing them proves nothing and the ritual is theatre"
    );

    // 4. The user confirms on the host, which releases the response.
    let _ = host.confirm_manual(host_code, NOW + 3);
    let completed = host.advance(&broker, NOW + 4);
    assert!(
        matches!(completed, InitiatorEvent::Completed { .. }),
        "expected Completed after confirmation, got {completed:?}"
    );
    assert_eq!(host.phase(), InitiatorPhase::Completed);

    // 5. The joiner collects the response it can actually decrypt.
    let ready = joiner.advance(&broker, NOW + 5);
    assert!(
        matches!(ready, ResponderEvent::ResponseReady),
        "the joiner must decrypt the host's response, got {ready:?}"
    );

    let response = joiner
        .take_response()
        .expect("ResponseReady means a decrypted response is waiting");
    assert_eq!(
        response.master_seed(),
        &master_seed,
        "the joiner must end up holding the identity's master seed — this \
         is what makes it the same identity on a second device"
    );
    assert_eq!(response.display_name(), "Alice");
}

/// The same ceremony with no relay at all, over ADR-070's local
/// rendezvous. This only became possible once the joiner opened its
/// return channel through the broker: `SingleCeremonyRendezvous` answers
/// `UnknownCode` for a code nobody offered, so a self-minted channel was
/// unreachable there as well as at the relay.
// @scenario: device_management :: two devices link over a local rendezvous
#[test]
fn both_machines_complete_a_link_over_a_local_rendezvous() {
    use std::sync::Arc;
    use vauchi_app::orchestrator::local_rendezvous::{
        LocalDeviceLinkBroker, SingleCeremonyRendezvous,
    };

    let master_seed = [0x77u8; 32];
    let (initiator, _identity) = initiator_for("Alice", master_seed);
    let rendezvous = Arc::new(SingleCeremonyRendezvous::new());
    let host_broker = LocalDeviceLinkBroker::new("100001".to_string(), Arc::clone(&rendezvous));
    let joiner_broker = LocalDeviceLinkBroker::new("100002".to_string(), Arc::clone(&rendezvous));

    let mut host = DeviceLinkInitiatorMachine::new(
        initiator,
        "alice-identity".to_string(),
        TIMEOUT_SECS,
        None,
    );
    let _ = host.advance(&host_broker, NOW);
    let invitation = host
        .join_invitation()
        .expect("a QR-ready host must expose an invitation");

    let mut joiner =
        DeviceLinkResponderMachine::new(invitation, "New Phone".to_string(), TIMEOUT_SECS)
            .expect("invitation parses");
    let joiner_code = match joiner.advance(&joiner_broker, NOW + 1) {
        ResponderEvent::RequestPosted { confirmation_code } => confirmation_code,
        other => panic!("expected RequestPosted over the local rendezvous, got {other:?}"),
    };

    let host_code = match host.advance(&host_broker, NOW + 2) {
        InitiatorEvent::ConfirmationRequired {
            confirmation_code, ..
        } => confirmation_code,
        other => panic!("expected ConfirmationRequired, got {other:?}"),
    };
    assert_eq!(host_code, joiner_code, "the codes must agree off-relay too");

    let _ = host.confirm_manual(host_code, NOW + 3);
    let completed = host.advance(&host_broker, NOW + 4);
    assert!(
        matches!(completed, InitiatorEvent::Completed { .. }),
        "expected Completed with no relay involved, got {completed:?}"
    );

    let ready = joiner.advance(&joiner_broker, NOW + 5);
    assert!(
        matches!(ready, ResponderEvent::ResponseReady),
        "the joiner must decrypt the response off-relay, got {ready:?}"
    );
    assert_eq!(
        joiner
            .take_response()
            .expect("a ready joiner holds a response")
            .master_seed(),
        &master_seed
    );
}

/// The same ceremony again, but the joiner reaches the host **across a
/// socket** rather than sharing its rendezvous in-process. Everything
/// before this shared a `SingleCeremonyRendezvous` by `Arc`, so no test had
/// yet put a transport between the two machines — the wire format, the host
/// listener, and the joiner broker were each covered alone.
///
/// The host mints its own code here; only the initiator knows it in
/// advance, and the joiner learns the response channel through the
/// invitation exactly as it would from a scanned QR.
// @scenario: device_management :: two devices link over a local socket
#[test]
fn both_machines_complete_a_link_over_a_socket_with_no_relay() {
    use std::sync::Arc;
    use std::time::Duration;
    use vauchi_app::orchestrator::local_client::RemoteRendezvousBroker;
    use vauchi_app::orchestrator::local_listener::{ListenerRuntime, LocalRendezvousListener};
    use vauchi_app::orchestrator::local_rendezvous::{
        LocalDeviceLinkBroker, SingleCeremonyRendezvous,
    };
    use vauchi_core::monotonic::SystemMonotonicClock;
    use vauchi_core::rng::OsSecureRng;
    use vauchi_core::sleeper::SystemSleeper;

    let master_seed = [0x33u8; 32];
    let (initiator, _identity) = initiator_for("Alice", master_seed);

    // The host owns the rendezvous and also serves it on a socket.
    let rendezvous = Arc::new(SingleCeremonyRendezvous::new());
    let listener = LocalRendezvousListener::bind(
        Arc::clone(&rendezvous),
        ListenerRuntime {
            rng: OsSecureRng::shared(),
            clock: SystemMonotonicClock::shared(),
            sleeper: SystemSleeper::shared(),
        },
        Duration::from_secs(30),
        Duration::from_millis(200),
    )
    .expect("host binds");

    let host_broker = LocalDeviceLinkBroker::new("100001".to_string(), Arc::clone(&rendezvous));
    let joiner_broker = RemoteRendezvousBroker::new(
        listener.addr(),
        Duration::from_millis(500),
        Duration::from_millis(500),
    );

    let mut host = DeviceLinkInitiatorMachine::new(
        initiator,
        "alice-identity".to_string(),
        TIMEOUT_SECS,
        None,
    );
    let _ = host.advance(&host_broker, NOW);
    let invitation = host
        .join_invitation()
        .expect("a QR-ready host must expose an invitation");

    let mut joiner =
        DeviceLinkResponderMachine::new(invitation, "New Phone".to_string(), TIMEOUT_SECS)
            .expect("invitation parses");
    let joiner_code = match joiner.advance(&joiner_broker, NOW + 1) {
        ResponderEvent::RequestPosted { confirmation_code } => confirmation_code,
        other => panic!("expected RequestPosted over the socket, got {other:?}"),
    };

    let host_code = match host.advance(&host_broker, NOW + 2) {
        InitiatorEvent::ConfirmationRequired {
            confirmation_code, ..
        } => confirmation_code,
        other => panic!("expected ConfirmationRequired, got {other:?}"),
    };
    assert_eq!(
        host_code, joiner_code,
        "the codes must agree across a transport, not just in-process"
    );

    let _ = host.confirm_manual(host_code, NOW + 3);
    let completed = host.advance(&host_broker, NOW + 4);
    assert!(
        matches!(completed, InitiatorEvent::Completed { .. }),
        "expected Completed over the socket, got {completed:?}"
    );

    let ready = joiner.advance(&joiner_broker, NOW + 5);
    assert!(
        matches!(ready, ResponderEvent::ResponseReady),
        "the joiner must decrypt the response received over the socket, got {ready:?}"
    );
    assert_eq!(
        joiner
            .take_response()
            .expect("a ready joiner holds a response")
            .master_seed(),
        &master_seed,
        "the seed must survive the whole ceremony over a real transport"
    );
}

// @scenario: device_management :: a joiner that never confirms cannot obtain the seed
#[test]
fn the_joiner_gets_nothing_until_the_user_confirms_on_the_host() {
    let (initiator, _identity) = initiator_for("Alice", [0x5Au8; 32]);
    let broker = RelayLikeBroker::new();
    let mut host = DeviceLinkInitiatorMachine::new(
        initiator,
        "alice-identity".to_string(),
        TIMEOUT_SECS,
        None,
    );

    let _ = host.advance(&broker, NOW);
    let invitation = host.join_invitation().expect("invitation");
    let mut joiner =
        DeviceLinkResponderMachine::new(invitation, "New Phone".to_string(), TIMEOUT_SECS)
            .expect("invitation parses");

    let _ = joiner.advance(&broker, NOW + 1);
    let _ = host.advance(&broker, NOW + 2); // ConfirmationRequired — not confirmed

    // Poll the joiner without the host ever confirming.
    let stalled = joiner.advance(&broker, NOW + 3);
    assert!(
        matches!(stalled, ResponderEvent::None),
        "without confirmation there is nothing to collect, got {stalled:?}"
    );
    assert!(
        joiner.take_response().is_none(),
        "proximity confirmation is what gates the master seed (ADR-035); a \
         joiner that skipped it must hold nothing"
    );
}

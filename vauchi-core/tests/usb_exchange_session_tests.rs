// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for USB/TCP direct transport exchange sessions.
//!
//! Verifies the ADR-031 command/event flow: session emits `DirectSend`,
//! frontend executes TCP exchange, reports `DirectPayloadReceived`.

#![cfg(feature = "testing")]

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::command::{ExchangeCommand, ExchangeHardwareEvent};
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession, ExchangeState};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::exchange::{ManualConfirmationVerifier, ProximityConfidence, UsbRole};
use vauchi_core::identity::Identity;
use vauchi_core::types::ExchangeTransport;

fn create_identity(name: &str) -> Identity {
    Identity::create(name)
}

fn create_card(identity: &Identity) -> ContactCard {
    ContactCard::new(identity.display_name())
}

// ── Session construction ───────────────────────────────────────

// @internal
#[test]
fn new_usb_session_starts_in_awaiting_direct_payload() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    assert!(matches!(
        session.state(),
        ExchangeState::AwaitingDirectPayload { .. }
    ));
    assert_eq!(session.transport(), ExchangeTransport::Usb);
}

// @internal
#[test]
fn new_usb_session_emits_direct_send_command() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    assert!(
        matches!(&commands[0], ExchangeCommand::DirectSend { payload, is_initiator }
            if !payload.is_empty() && *is_initiator),
        "should emit DirectSend with non-empty payload"
    );
}

// ── Full exchange ceremony via command/event ────────────────────

// @internal
#[test]
fn full_usb_exchange_ceremony_via_commands() {
    let alice_id = create_identity("Alice");
    let alice_card = create_card(&alice_id);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    let mut alice_session = ExchangeSession::new_usb(
        alice_id,
        alice_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    // Step 1: Both sessions emit DirectSend commands
    alice_session.emit_initial_commands();
    bob_session.emit_initial_commands();

    let alice_commands = alice_session.drain_commands();
    let bob_commands = bob_session.drain_commands();

    let alice_payload = match &alice_commands[0] {
        ExchangeCommand::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };
    let bob_payload = match &bob_commands[0] {
        ExchangeCommand::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    // Step 2: Frontend exchanges payloads over TCP (simulated here by swapping)
    // Alice receives Bob's payload, Bob receives Alice's payload

    // Step 3: Frontend reports received payloads as hardware events
    alice_session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived { data: bob_payload })
        .expect("alice process payload");
    bob_session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived {
            data: alice_payload,
        })
        .expect("bob process payload");

    // Both should be in AwaitingKeyAgreement
    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));
    assert!(matches!(
        bob_session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));

    // Step 4: Key agreement
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("alice key agreement");
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("bob key agreement");

    assert!(matches!(
        alice_session.state(),
        ExchangeState::AwaitingCardExchange { .. }
    ));

    // Step 5: Set proximity confidence (USB = Physical = High)
    alice_session
        .apply(ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::High,
        })
        .expect("alice proximity");
    bob_session
        .apply(ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::High,
        })
        .expect("bob proximity");

    // Step 6: Complete exchange with cards
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .expect("alice complete");
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .expect("bob complete");

    assert!(alice_session.is_complete());
    assert!(bob_session.is_complete());

    let alice_contact = alice_session.extract_contact();
    let bob_contact = bob_session.extract_contact();
    assert!(alice_contact.is_some(), "Alice should have a contact");
    assert!(bob_contact.is_some(), "Bob should have a contact");
}

// ── Error cases ────────────────────────────────────────────────

// @internal
#[test]
fn usb_self_exchange_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);

    let bytes = identity.to_storage_bytes();
    let identity2 = Identity::from_storage_bytes(&bytes).expect("roundtrip");

    let mut session1 = ExchangeSession::new_usb(
        identity,
        card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );
    let mut session2 = ExchangeSession::new_usb(
        identity2,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    session2.emit_initial_commands();
    let commands = session2.drain_commands();
    let payload = match &commands[0] {
        ExchangeCommand::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    let result = session1
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived { data: payload });
    assert!(result.is_err(), "Self-exchange should be rejected");
}

// @internal
#[test]
fn usb_invalid_payload_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);

    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    let result = session.apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived {
        data: b"garbage-not-a-valid-qr-payload".to_vec(),
    });
    assert!(result.is_err(), "Invalid payload should be rejected");
}

// @internal
#[test]
fn usb_direct_payload_in_wrong_state_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    bob_session.emit_initial_commands();
    let bob_payload = match &bob_session.drain_commands()[0] {
        ExchangeCommand::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    // Process payload once (valid)
    session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived {
            data: bob_payload.clone(),
        })
        .expect("first process should succeed");

    // Try to process again (wrong state — now in AwaitingKeyAgreement)
    let result = session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived { data: bob_payload });
    assert!(
        result.is_err(),
        "Should reject DirectPayloadReceived in AwaitingKeyAgreement state"
    );
}

// ── UsbRole: initiator/responder ───────────────────────────────

// @internal
#[test]
fn usb_initiator_emits_is_initiator_true() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    match &commands[0] {
        ExchangeCommand::DirectSend { is_initiator, .. } => {
            assert!(*is_initiator, "Initiator role must set is_initiator: true");
        }
        other => panic!("expected DirectSend, got {:?}", other),
    }
}

// @internal
#[test]
fn usb_responder_emits_is_initiator_false() {
    let identity = create_identity("Bob");
    let card = create_card(&identity);
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Responder,
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    match &commands[0] {
        ExchangeCommand::DirectSend { is_initiator, .. } => {
            assert!(
                !*is_initiator,
                "Responder role must set is_initiator: false"
            );
        }
        other => panic!("expected DirectSend, got {:?}", other),
    }
}

// @internal
#[test]
fn full_usb_exchange_over_tcp_loopback() {
    // Create two sessions — Alice is initiator (desktop), Bob is responder (phone)
    let alice_id = create_identity("Alice");
    let alice_card = create_card(&alice_id);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    let mut alice_session = ExchangeSession::new_usb(
        alice_id,
        alice_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Responder,
    );

    // Emit initial commands from both sessions
    alice_session.emit_initial_commands();
    bob_session.emit_initial_commands();
    let alice_cmds = alice_session.drain_commands();
    let bob_cmds = bob_session.drain_commands();

    // Extract payloads and initiator flags
    let (alice_payload, alice_init) = match &alice_cmds[0] {
        ExchangeCommand::DirectSend {
            payload,
            is_initiator,
        } => (payload.clone(), *is_initiator),
        other => panic!("expected DirectSend from alice, got {:?}", other),
    };
    let (bob_payload, bob_init) = match &bob_cmds[0] {
        ExchangeCommand::DirectSend {
            payload,
            is_initiator,
        } => (payload.clone(), *is_initiator),
        other => panic!("expected DirectSend from bob, got {:?}", other),
    };

    // Verify roles
    assert!(alice_init, "alice should be initiator");
    assert!(!bob_init, "bob should be responder");

    // TCP loopback: Bob listens, Alice connects
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // Bob runs in a thread (responder: recv first, then send)
    let bob_handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut t = TcpDirectTransport::physical(stream);
        t.exchange(&bob_payload, bob_init).unwrap()
    });

    // Alice connects and exchanges (initiator: send first, then recv)
    let alice_stream = TcpStream::connect(addr).unwrap();
    let mut alice_t = TcpDirectTransport::physical(alice_stream);
    let bob_received = alice_t.exchange(&alice_payload, alice_init).unwrap();
    let alice_received = bob_handle.join().unwrap();

    // Feed received payloads back to sessions
    alice_session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived { data: bob_received })
        .expect("alice processes bob payload");
    bob_session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived {
            data: alice_received,
        })
        .expect("bob processes alice payload");

    // Both should be in AwaitingKeyAgreement
    assert!(
        matches!(
            alice_session.state(),
            ExchangeState::AwaitingKeyAgreement { .. }
        ),
        "alice should be in AwaitingKeyAgreement"
    );
    assert!(
        matches!(
            bob_session.state(),
            ExchangeState::AwaitingKeyAgreement { .. }
        ),
        "bob should be in AwaitingKeyAgreement"
    );

    // Complete key agreement
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("alice key agreement");
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("bob key agreement");

    // Both should now be in AwaitingCardExchange
    assert!(
        matches!(
            alice_session.state(),
            ExchangeState::AwaitingCardExchange { .. }
        ),
        "alice should be in AwaitingCardExchange"
    );
    assert!(
        matches!(
            bob_session.state(),
            ExchangeState::AwaitingCardExchange { .. }
        ),
        "bob should be in AwaitingCardExchange"
    );
}

// @internal
#[test]
fn usb_direct_payload_on_qr_session_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    // Create a QR session, not USB
    let mut session = ExchangeSession::new_qr(identity, card, ManualConfirmationVerifier::new());
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
    );

    bob_session.emit_initial_commands();
    let bob_payload = match &bob_session.drain_commands()[0] {
        ExchangeCommand::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    let result = session
        .apply_hardware_event(ExchangeHardwareEvent::DirectPayloadReceived { data: bob_payload });
    assert!(
        result.is_err(),
        "DirectPayloadReceived should be rejected on QR session"
    );
}

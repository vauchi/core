// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for USB/TCP direct transport exchange sessions.
//!
//! Verifies the ADR-031 command/event flow: session emits `DirectSend`,
//! frontend executes TCP exchange, reports `DirectPayloadReceived`.

#![cfg(feature = "testing")]

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::command::{ExchangeCommand, ExchangeHardwareEvent};
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession, ExchangeState};
use vauchi_core::exchange::{ManualConfirmationVerifier, ProximityConfidence};
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
    let session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

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
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

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
    );
    let mut bob_session =
        ExchangeSession::new_usb(bob_id, bob_card.clone(), ManualConfirmationVerifier::new());

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

    let mut session1 =
        ExchangeSession::new_usb(identity, card.clone(), ManualConfirmationVerifier::new());
    let mut session2 = ExchangeSession::new_usb(identity2, card, ManualConfirmationVerifier::new());

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

    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

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

    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());
    let mut bob_session =
        ExchangeSession::new_usb(bob_id, bob_card, ManualConfirmationVerifier::new());

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

// @internal
#[test]
fn usb_direct_payload_on_qr_session_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    // Create a QR session, not USB
    let mut session = ExchangeSession::new_qr(identity, card, ManualConfirmationVerifier::new());
    let mut bob_session =
        ExchangeSession::new_usb(bob_id, bob_card, ManualConfirmationVerifier::new());

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

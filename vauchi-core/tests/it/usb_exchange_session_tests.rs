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

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::exchange::ExchangeError;
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession, ExchangeState};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::exchange::{ManualConfirmationVerifier, ProximityConfidence, UsbRole};
use vauchi_core::identity::Identity;
use vauchi_core::types::ExchangeTransport;
use vauchi_core::{Command, Event};

fn create_identity(name: &str) -> Identity {
    Identity::create(name, 0)
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
        vauchi_core::clock::SystemClock::shared(),
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
        vauchi_core::clock::SystemClock::shared(),
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    assert!(
        matches!(&commands[0], Command::DirectSend { payload, is_initiator }
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
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Step 1: Both sessions emit DirectSend commands
    alice_session.emit_initial_commands();
    bob_session.emit_initial_commands();

    let alice_commands = alice_session.drain_commands();
    let bob_commands = bob_session.drain_commands();

    let alice_payload = match &alice_commands[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };
    let bob_payload = match &bob_commands[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    // Step 2: Frontend exchanges payloads over TCP (simulated here by swapping)

    // Step 3: Frontend reports received payloads as hardware events
    alice_session
        .apply_hardware_event(Event::DirectPayloadReceived { data: bob_payload })
        .expect("alice process payload");
    bob_session
        .apply_hardware_event(Event::DirectPayloadReceived {
            data: alice_payload,
        })
        .expect("bob process payload");

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
    let identity2 = Identity::from_storage_bytes(&bytes, 0).expect("roundtrip");

    let mut session1 = ExchangeSession::new_usb(
        identity,
        card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut session2 = ExchangeSession::new_usb(
        identity2,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    session2.emit_initial_commands();
    let commands = session2.drain_commands();
    let payload = match &commands[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    let result = session1.apply_hardware_event(Event::DirectPayloadReceived { data: payload });
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
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = session.apply_hardware_event(Event::DirectPayloadReceived {
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
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.emit_initial_commands();
    let bob_payload = match &bob_session.drain_commands()[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    // Process payload once (valid)
    session
        .apply_hardware_event(Event::DirectPayloadReceived {
            data: bob_payload.clone(),
        })
        .expect("first process should succeed");

    // Try to process again (wrong state — now in AwaitingKeyAgreement)
    let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: bob_payload });
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
        vauchi_core::clock::SystemClock::shared(),
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    match &commands[0] {
        Command::DirectSend { is_initiator, .. } => {
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
        vauchi_core::clock::SystemClock::shared(),
    );

    session.emit_initial_commands();
    let commands = session.drain_commands();

    assert_eq!(commands.len(), 1, "should emit exactly one command");
    match &commands[0] {
        Command::DirectSend { is_initiator, .. } => {
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
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card.clone(),
        ManualConfirmationVerifier::new(),
        UsbRole::Responder,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Emit initial commands from both sessions
    alice_session.emit_initial_commands();
    bob_session.emit_initial_commands();
    let alice_cmds = alice_session.drain_commands();
    let bob_cmds = bob_session.drain_commands();

    // Extract payloads and initiator flags
    let (alice_payload, alice_init) = match &alice_cmds[0] {
        Command::DirectSend {
            payload,
            is_initiator,
        } => (payload.clone(), *is_initiator),
        other => panic!("expected DirectSend from alice, got {:?}", other),
    };
    let (bob_payload, bob_init) = match &bob_cmds[0] {
        Command::DirectSend {
            payload,
            is_initiator,
        } => (payload.clone(), *is_initiator),
        other => panic!("expected DirectSend from bob, got {:?}", other),
    };

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
        .apply_hardware_event(Event::DirectPayloadReceived { data: bob_received })
        .expect("alice processes bob payload");
    bob_session
        .apply_hardware_event(Event::DirectPayloadReceived {
            data: alice_received,
        })
        .expect("bob processes alice payload");

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

    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("alice key agreement");
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("bob key agreement");

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

    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_usb(
        bob_id,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.emit_initial_commands();
    let bob_payload = match &bob_session.drain_commands()[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: bob_payload });
    assert!(
        result.is_err(),
        "DirectPayloadReceived should be rejected on QR session"
    );
}

// ── Real card round-trip (no hand-feeding) ──────────────────────
//
// `2026-06-05-usb-card-exchange-protocol`: each session learns the *peer's*
// ContactCard ONLY via the encrypted `DirectSendCard` / `DirectCardReceived`
// round — proving the wire actually carries the card. (The ceremony test
// above hand-feeds both cards, so it never exercised this.)

fn card_with_email(identity: &Identity, email: &str) -> ContactCard {
    let mut card = ContactCard::new(identity.display_name());
    card.add_field(ContactField::new(FieldType::Email, "Email", email, 0))
        .expect("add_field");
    card
}

/// Emit our `DirectSend` and return its payload (the QR/key leg).
fn direct_send_payload(session: &mut ExchangeSession) -> Vec<u8> {
    session.emit_initial_commands();
    match &session.drain_commands()[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {other:?}"),
    }
}

/// Receive the peer's QR payload + perform key agreement, then return our
/// encrypted-card ciphertext (the second leg). Leaves the session in
/// `AwaitingCardExchange` with proximity High (USB is physical).
fn key_agree_and_get_card_ciphertext(
    session: &mut ExchangeSession,
    peer_payload: Vec<u8>,
) -> Vec<u8> {
    session
        .apply_hardware_event(Event::DirectPayloadReceived { data: peer_payload })
        .expect("process peer payload");
    session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("key agreement");
    assert!(
        matches!(session.state(), ExchangeState::AwaitingCardExchange { .. }),
        "USB key agreement lands in AwaitingCardExchange"
    );
    session
        .drain_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::DirectSendCard { ciphertext, .. } => Some(ciphertext),
            _ => None,
        })
        .expect("PerformKeyAgreement must emit DirectSendCard for USB")
}

// @internal
#[test]
fn usb_card_round_trip_completes_with_peer_card() {
    let alice_id = create_identity("Alice");
    let bob_id = create_identity("Bob");
    let alice_card = card_with_email(&alice_id, "alice@example.com");
    let bob_card = card_with_email(&bob_id, "bob@example.com");

    let mut alice = ExchangeSession::new_usb(
        alice_id,
        alice_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob = ExchangeSession::new_usb(
        bob_id,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Responder,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Leg 1: swap the key-bearing QR payloads.
    let alice_payload = direct_send_payload(&mut alice);
    let bob_payload = direct_send_payload(&mut bob);

    // Key agreement → each emits its encrypted card.
    let alice_card_ct = key_agree_and_get_card_ciphertext(&mut alice, bob_payload);
    let bob_card_ct = key_agree_and_get_card_ciphertext(&mut bob, alice_payload);

    // Leg 2: swap the encrypted cards. Each side decrypts the PEER's card under
    // the agreed shared key and completes — no card was ever hand-fed.
    alice
        .apply_hardware_event(Event::DirectCardReceived {
            ciphertext: bob_card_ct,
        })
        .expect("alice receives bob's card");
    bob.apply_hardware_event(Event::DirectCardReceived {
        ciphertext: alice_card_ct,
    })
    .expect("bob receives alice's card");

    assert!(alice.is_complete(), "alice completed");
    assert!(bob.is_complete(), "bob completed");

    let alice_contact = alice.extract_contact().expect("alice contact");
    let bob_contact = bob.extract_contact().expect("bob contact");

    // Each saved the OTHER's full card (display name + the email field), proving
    // the card crossed the wire encrypted — not just QR metadata.
    assert_eq!(alice_contact.card().display_name(), "Bob");
    assert_eq!(bob_contact.card().display_name(), "Alice");
    assert!(
        alice_contact
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "bob@example.com"),
        "Alice must receive Bob's full card via the encrypted round-trip"
    );
    assert!(
        bob_contact
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "alice@example.com"),
        "Bob must receive Alice's full card via the encrypted round-trip"
    );
}

// @internal
#[test]
fn usb_tampered_card_ciphertext_is_rejected() {
    let alice_id = create_identity("Alice");
    let bob_id = create_identity("Bob");
    let mut alice = ExchangeSession::new_usb(
        alice_id,
        card_with_email(&create_identity("Alice"), "alice@example.com"),
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob = ExchangeSession::new_usb(
        bob_id,
        card_with_email(&create_identity("Bob"), "bob@example.com"),
        ManualConfirmationVerifier::new(),
        UsbRole::Responder,
        vauchi_core::clock::SystemClock::shared(),
    );

    let alice_payload = direct_send_payload(&mut alice);
    let bob_payload = direct_send_payload(&mut bob);
    let _ = key_agree_and_get_card_ciphertext(&mut alice, bob_payload);
    let mut bob_card_ct = key_agree_and_get_card_ciphertext(&mut bob, alice_payload);

    // Flip a byte of the ciphertext → AEAD authentication must fail.
    *bob_card_ct.last_mut().expect("non-empty ciphertext") ^= 0xFF;

    let result = alice.apply_hardware_event(Event::DirectCardReceived {
        ciphertext: bob_card_ct,
    });
    assert!(
        matches!(result, Err(ExchangeError::UsbDecryptionFailed)),
        "tampered card ciphertext must be rejected, got {result:?}"
    );
    assert!(
        !alice.is_complete(),
        "a rejected card must not complete the exchange"
    );
}

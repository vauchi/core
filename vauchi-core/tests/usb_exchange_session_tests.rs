// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for USB/TCP direct transport exchange sessions.
//!
//! Verifies the full exchange ceremony over `TcpDirectTransport`:
//! payload generation → transport exchange → key agreement → card exchange.

#![cfg(feature = "testing")]

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession, ExchangeState};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::exchange::{DirectTransport, ManualConfirmationVerifier, ProximityConfidence};
use vauchi_core::identity::Identity;
use vauchi_core::types::ExchangeTransport;

/// Helper: create a connected pair of TCP streams on loopback.
fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

fn create_identity(name: &str) -> Identity {
    Identity::create(name)
}

fn create_card(identity: &Identity) -> ContactCard {
    ContactCard::new(identity.display_name())
}

// ── Session construction ───────────────────────────────────────

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

#[test]
fn new_usb_session_provides_exchange_payload() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let payload = session.our_exchange_payload();
    assert!(
        payload.is_some(),
        "USB session should provide exchange payload"
    );
    assert!(!payload.unwrap().is_empty(), "payload should not be empty");
}

// ── Payload exchange over DirectTransport ──────────────────────

#[test]
fn usb_exchange_payload_roundtrip_over_tcp() {
    let alice_id = create_identity("Alice");
    let alice_card = create_card(&alice_id);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    let mut alice_session =
        ExchangeSession::new_usb(alice_id, alice_card, ManualConfirmationVerifier::new());
    let mut bob_session =
        ExchangeSession::new_usb(bob_id, bob_card, ManualConfirmationVerifier::new());

    let alice_payload = alice_session.our_exchange_payload().unwrap();
    let bob_payload = bob_session.our_exchange_payload().unwrap();

    // Exchange payloads over TCP
    let (client, server) = loopback_pair();
    let bob_payload_clone = bob_payload.clone();
    let bob_handle = thread::spawn(move || {
        let mut transport = TcpDirectTransport::physical(server);
        transport.exchange(bob_payload_clone.as_bytes(), false)
    });

    let mut alice_transport = TcpDirectTransport::physical(client);
    let received_by_alice = alice_transport
        .exchange(alice_payload.as_bytes(), true)
        .expect("alice exchange");

    let received_by_bob = bob_handle
        .join()
        .expect("bob thread")
        .expect("bob exchange");

    // Verify payloads were exchanged correctly
    assert_eq!(received_by_alice, bob_payload.as_bytes());
    assert_eq!(received_by_bob, alice_payload.as_bytes());
}

// ── Full exchange ceremony ─────────────────────────────────────

#[test]
fn full_usb_exchange_ceremony() {
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

    // Step 1: Get payloads
    let alice_payload = alice_session.our_exchange_payload().unwrap();
    let bob_payload = bob_session.our_exchange_payload().unwrap();

    // Step 2: Exchange over TCP (simulating USB cable)
    let (client, server) = loopback_pair();
    let bob_payload_for_tcp = bob_payload.clone();
    let bob_handle = thread::spawn(move || {
        let mut transport = TcpDirectTransport::physical(server);
        transport.exchange(bob_payload_for_tcp.as_bytes(), false)
    });

    let mut alice_transport = TcpDirectTransport::physical(client);
    let received_by_alice = alice_transport
        .exchange(alice_payload.as_bytes(), true)
        .expect("tcp exchange");
    let received_by_bob = bob_handle.join().expect("thread").expect("tcp exchange");

    let alice_received_str = String::from_utf8(received_by_alice).expect("valid utf8");
    let bob_received_str = String::from_utf8(received_by_bob).expect("valid utf8");

    // Step 3: Feed received payloads to sessions
    alice_session
        .apply(ExchangeEvent::DirectPayloadReceived {
            their_payload: alice_received_str,
        })
        .expect("alice process payload");
    bob_session
        .apply(ExchangeEvent::DirectPayloadReceived {
            their_payload: bob_received_str,
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

    // Verify contacts were created
    let alice_contact = alice_session.extract_contact();
    let bob_contact = bob_session.extract_contact();
    assert!(alice_contact.is_some(), "Alice should have a contact");
    assert!(bob_contact.is_some(), "Bob should have a contact");
}

// ── Error cases ────────────────────────────────────────────────

#[test]
fn usb_self_exchange_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);

    // Roundtrip through storage bytes to get a second copy of the same identity
    let bytes = identity.to_storage_bytes();
    let identity2 = Identity::from_storage_bytes(&bytes).expect("roundtrip");

    let mut session1 =
        ExchangeSession::new_usb(identity, card.clone(), ManualConfirmationVerifier::new());
    let session2 = ExchangeSession::new_usb(identity2, card, ManualConfirmationVerifier::new());

    let payload = session2.our_exchange_payload().unwrap();

    let result = session1.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: payload,
    });
    assert!(result.is_err(), "Self-exchange should be rejected");
}

#[test]
fn usb_invalid_payload_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);

    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: "garbage-not-a-valid-qr-payload".to_string(),
    });
    assert!(result.is_err(), "Invalid payload should be rejected");
}

#[test]
fn usb_direct_payload_in_wrong_state_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());
    let bob_session = ExchangeSession::new_usb(bob_id, bob_card, ManualConfirmationVerifier::new());
    let bob_payload = bob_session.our_exchange_payload().unwrap();

    // Process payload once (valid)
    session
        .apply(ExchangeEvent::DirectPayloadReceived {
            their_payload: bob_payload.clone(),
        })
        .expect("first process should succeed");

    // Try to process again (wrong state — now in AwaitingKeyAgreement)
    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: bob_payload,
    });
    assert!(
        result.is_err(),
        "Should reject DirectPayloadReceived in AwaitingKeyAgreement state"
    );
}

#[test]
fn usb_direct_payload_on_qr_session_is_rejected() {
    let identity = create_identity("Alice");
    let card = create_card(&identity);
    let bob_id = create_identity("Bob");
    let bob_card = create_card(&bob_id);

    // Create a QR session, not USB
    let mut session = ExchangeSession::new_qr(identity, card, ManualConfirmationVerifier::new());
    let bob_session = ExchangeSession::new_usb(bob_id, bob_card, ManualConfirmationVerifier::new());
    let bob_payload = bob_session.our_exchange_payload().unwrap();

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: bob_payload,
    });
    assert!(
        result.is_err(),
        "DirectPayloadReceived should be rejected on QR session"
    );
}

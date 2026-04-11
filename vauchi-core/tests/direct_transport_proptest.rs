// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based and adversarial tests for DirectTransport and USB exchange.
//!
//! CC-04: proptest for transport payload roundtrips.
//! CC-14: adversarial payloads (truncated, corrupted, max-length, unicode, null bytes).

#![cfg(feature = "testing")]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

use proptest::prelude::*;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::ManualConfirmationVerifier;
use vauchi_core::exchange::direct_transport::DirectTransport;
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession, ExchangeState};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::identity::Identity;

fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

// ── Property-based tests ───────────────────────────────────────

proptest! {
    /// Arbitrary payloads survive roundtrip over TcpDirectTransport.
    // @internal
    #[test]
    fn payload_roundtrip(payload in proptest::collection::vec(any::<u8>(), 1..8192)) {
        let (client, server) = loopback_pair();
        let payload_clone = payload.clone();

        let handle = thread::spawn(move || {
            let mut sender = TcpDirectTransport::physical(client);
            sender.send(&payload_clone)
        });

        let mut receiver = TcpDirectTransport::physical(server);
        let received = receiver.recv().unwrap();
        handle.join().unwrap().unwrap();

        prop_assert_eq!(payload, received);
    }

    /// Bidirectional exchange preserves both payloads for arbitrary data.
    // @internal
    #[test]
    fn exchange_roundtrip(
        alice_data in proptest::collection::vec(any::<u8>(), 1..4096),
        bob_data in proptest::collection::vec(any::<u8>(), 1..4096),
    ) {
        let (client, server) = loopback_pair();
        let bob_data_clone = bob_data.clone();

        let handle = thread::spawn(move || {
            let mut bob = TcpDirectTransport::physical(server);
            bob.exchange(&bob_data_clone, false)
        });

        let mut alice = TcpDirectTransport::physical(client);
        let received_by_alice = alice.exchange(&alice_data, true).unwrap();
        let received_by_bob = handle.join().unwrap().unwrap();

        prop_assert_eq!(bob_data, received_by_alice);
        prop_assert_eq!(alice_data, received_by_bob);
    }
}

// ── Adversarial tests (CC-14) ──────────────────────────────────

// @internal
#[test]
fn adversarial_empty_direct_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: String::new(),
    });
    assert!(result.is_err(), "Empty payload should be rejected");
}

// @internal
#[test]
fn adversarial_null_bytes_in_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: "\0\0\0\0\0\0\0\0".to_string(),
    });
    assert!(result.is_err(), "Null bytes payload should be rejected");
}

// @internal
#[test]
fn adversarial_unicode_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: "🔥💀🎃".repeat(100),
    });
    assert!(result.is_err(), "Unicode garbage should be rejected");
}

// @internal
#[test]
fn adversarial_max_length_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: "A".repeat(100_000),
    });
    assert!(result.is_err(), "100KB garbage payload should be rejected");
}

// @internal
#[test]
fn adversarial_truncated_valid_payload() {
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new(bob.display_name());
    let bob_session = ExchangeSession::new_usb(bob, bob_card, ManualConfirmationVerifier::new());
    let valid_payload = bob_session.our_exchange_payload().unwrap();

    // Truncate at various points
    for truncate_at in [1, 10, 50, valid_payload.len() / 2, valid_payload.len() - 1] {
        let identity = Identity::create("Alice");
        let card = ContactCard::new(identity.display_name());
        let mut session =
            ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

        let truncated = &valid_payload[..truncate_at];
        let result = session.apply(ExchangeEvent::DirectPayloadReceived {
            their_payload: truncated.to_string(),
        });
        assert!(
            result.is_err(),
            "Truncated payload at {} bytes should be rejected",
            truncate_at
        );
    }
}

// @internal
#[test]
fn adversarial_corrupted_valid_payload() {
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new(bob.display_name());
    let bob_session = ExchangeSession::new_usb(bob, bob_card, ManualConfirmationVerifier::new());
    let valid_payload = bob_session.our_exchange_payload().unwrap();

    // Flip bits at various positions
    let mut corrupted = valid_payload.clone().into_bytes();
    for pos in [0, 10, corrupted.len() / 2, corrupted.len() - 1] {
        corrupted[pos] ^= 0xFF;
    }

    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(identity, card, ManualConfirmationVerifier::new());

    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: String::from_utf8_lossy(&corrupted).to_string(),
    });
    assert!(result.is_err(), "Corrupted payload should be rejected");
}

// @internal
#[test]
fn adversarial_replay_same_payload_twice() {
    let alice = Identity::create("Alice");
    let alice_card = ContactCard::new(alice.display_name());
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new(bob.display_name());

    let bob_session = ExchangeSession::new_usb(bob, bob_card, ManualConfirmationVerifier::new());
    let bob_payload = bob_session.our_exchange_payload().unwrap();

    let mut session =
        ExchangeSession::new_usb(alice, alice_card, ManualConfirmationVerifier::new());

    // First apply succeeds
    session
        .apply(ExchangeEvent::DirectPayloadReceived {
            their_payload: bob_payload.clone(),
        })
        .expect("first should succeed");

    assert!(matches!(
        session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));

    // Second apply fails (wrong state)
    let result = session.apply(ExchangeEvent::DirectPayloadReceived {
        their_payload: bob_payload,
    });
    assert!(
        result.is_err(),
        "Replayed payload should be rejected (wrong state)"
    );
}

// @internal
#[test]
fn adversarial_tcp_invalid_magic_over_direct_transport() {
    let (mut client, server) = loopback_pair();

    // Send garbage with wrong magic bytes
    client.write_all(b"XXXX").expect("write magic");
    client.write_all(&[1]).expect("write version");
    client.write_all(&4u32.to_be_bytes()).expect("write len");
    client.write_all(b"test").expect("write payload");
    drop(client);

    let mut transport = TcpDirectTransport::physical(server);
    let result = transport.recv();
    assert!(
        result.is_err(),
        "Invalid magic should be rejected through DirectTransport"
    );
}

// @internal
#[test]
fn adversarial_tcp_wrong_version_over_direct_transport() {
    let (mut client, server) = loopback_pair();

    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[99]).expect("bad version");
    client.write_all(&4u32.to_be_bytes()).expect("len");
    client.write_all(b"test").expect("payload");
    drop(client);

    let mut transport = TcpDirectTransport::physical(server);
    let result = transport.recv();
    assert!(
        result.is_err(),
        "Wrong version should be rejected through DirectTransport"
    );
}

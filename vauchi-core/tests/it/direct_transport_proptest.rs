// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based and adversarial tests for TcpDirectTransport and USB exchange.
//!
//! CC-04: proptest for transport payload roundtrips.
//! CC-14: adversarial payloads (truncated, corrupted, max-length, unicode, null bytes).

#![cfg(feature = "testing")]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

use proptest::prelude::*;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::session::{ExchangeSession, ExchangeState};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::exchange::{ManualConfirmationVerifier, UsbRole};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

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
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: Vec::new() });
    assert!(result.is_err(), "Empty payload should be rejected");
}

// @internal
#[test]
fn adversarial_null_bytes_in_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = session.apply_hardware_event(Event::DirectPayloadReceived {
        data: b"\0\0\0\0\0\0\0\0".to_vec(),
    });
    assert!(result.is_err(), "Null bytes payload should be rejected");
}

// @internal
#[test]
fn adversarial_non_utf8_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Invalid UTF-8 sequence
    let result = session.apply_hardware_event(Event::DirectPayloadReceived {
        data: vec![0xFF, 0xFE, 0x80, 0x81, 0xC0],
    });
    assert!(result.is_err(), "Non-UTF-8 payload should be rejected");
}

// @internal
#[test]
fn adversarial_max_length_payload_rejected() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = session.apply_hardware_event(Event::DirectPayloadReceived {
        data: b"A".repeat(100_000),
    });
    assert!(result.is_err(), "100KB garbage payload should be rejected");
}

// @internal
#[test]
fn adversarial_truncated_valid_payload() {
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new(bob.display_name());
    let mut bob_session = ExchangeSession::new_usb(
        bob,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.emit_initial_commands();
    let valid_payload = match &bob_session.drain_commands()[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    for truncate_at in [1, 10, 50, valid_payload.len() / 2, valid_payload.len() - 1] {
        let identity = Identity::create("Alice");
        let card = ContactCard::new(identity.display_name());
        let mut session = ExchangeSession::new_usb(
            identity,
            card,
            ManualConfirmationVerifier::new(),
            UsbRole::Initiator,
            vauchi_core::clock::SystemClock::shared(),
        );

        let truncated = valid_payload[..truncate_at].to_vec();
        let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: truncated });
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
    let mut bob_session = ExchangeSession::new_usb(
        bob,
        bob_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    bob_session.emit_initial_commands();
    let valid_payload = match &bob_session.drain_commands()[0] {
        Command::DirectSend { payload, .. } => payload.clone(),
        other => panic!("expected DirectSend, got {:?}", other),
    };

    // Corrupt specific base64 characters (stay in ASCII range for valid UTF-8)
    let mut corrupted = valid_payload.clone();
    for pos in [0, 10, corrupted.len() / 2, corrupted.len() - 1] {
        // XOR with 0x20 flips case for ASCII letters, stays valid UTF-8
        corrupted[pos] ^= 0x20;
    }

    let identity = Identity::create("Alice");
    let card = ContactCard::new(identity.display_name());
    let mut session = ExchangeSession::new_usb(
        identity,
        card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: corrupted });
    assert!(result.is_err(), "Corrupted payload should be rejected");
}

// @internal
#[test]
fn adversarial_replay_same_payload_twice() {
    let alice = Identity::create("Alice");
    let alice_card = ContactCard::new(alice.display_name());
    let bob = Identity::create("Bob");
    let bob_card = ContactCard::new(bob.display_name());

    let mut bob_session = ExchangeSession::new_usb(
        bob,
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

    let mut session = ExchangeSession::new_usb(
        alice,
        alice_card,
        ManualConfirmationVerifier::new(),
        UsbRole::Initiator,
        vauchi_core::clock::SystemClock::shared(),
    );

    // First apply succeeds
    session
        .apply_hardware_event(Event::DirectPayloadReceived {
            data: bob_payload.clone(),
        })
        .expect("first should succeed");

    assert!(matches!(
        session.state(),
        ExchangeState::AwaitingKeyAgreement { .. }
    ));

    // Second apply fails (wrong state)
    let result = session.apply_hardware_event(Event::DirectPayloadReceived { data: bob_payload });
    assert!(
        result.is_err(),
        "Replayed payload should be rejected (wrong state)"
    );
}

// @internal
#[test]
fn adversarial_tcp_invalid_magic_over_transport() {
    let (mut client, server) = loopback_pair();

    client.write_all(b"XXXX").expect("write magic");
    client.write_all(&[1]).expect("write version");
    client.write_all(&4u32.to_be_bytes()).expect("write len");
    client.write_all(b"test").expect("write payload");
    drop(client);

    let mut transport = TcpDirectTransport::physical(server);
    let result = transport.recv();
    assert!(result.is_err(), "Invalid magic should be rejected");
}

// @internal
#[test]
fn adversarial_tcp_wrong_version_over_transport() {
    let (mut client, server) = loopback_pair();

    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[99]).expect("bad version");
    client.write_all(&4u32.to_be_bytes()).expect("len");
    client.write_all(b"test").expect("payload");
    drop(client);

    let mut transport = TcpDirectTransport::physical(server);
    let result = transport.recv();
    assert!(result.is_err(), "Wrong version should be rejected");
}

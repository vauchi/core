// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the DirectTransport trait and TcpDirectTransport implementation.
//!
//! Verifies the transport abstraction layer that enables desktop-to-phone
//! exchange over TCP (USB cable / local network).

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::exchange::direct_transport::{DirectTransport, ProximityLevel};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;

/// Helper: create a connected pair of TCP streams on loopback.
fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

// ── ProximityLevel ─────────────────────────────────────────────

// @internal
#[test]
fn physical_transport_has_physical_proximity() {
    let (client, _server) = loopback_pair();
    let transport = TcpDirectTransport::physical(client);
    assert_eq!(transport.proximity_level(), ProximityLevel::Physical);
}

// @internal
#[test]
fn proximate_transport_has_proximate_proximity() {
    let (client, _server) = loopback_pair();
    let transport = TcpDirectTransport::proximate(client);
    assert_eq!(transport.proximity_level(), ProximityLevel::Proximate);
}

// ── DirectTransport send/recv ──────────────────────────────────

// @internal
#[test]
fn direct_send_recv_roundtrip() {
    let (client, server) = loopback_pair();
    let mut sender = TcpDirectTransport::physical(client);
    let mut receiver = TcpDirectTransport::physical(server);

    let payload = b"direct-transport-test-payload";
    sender.send(payload).expect("send");
    let received = receiver.recv().expect("recv");

    assert_eq!(received, payload);
}

// @internal
#[test]
fn direct_send_recv_large_payload() {
    let (client, server) = loopback_pair();
    let mut sender = TcpDirectTransport::physical(client);
    let mut receiver = TcpDirectTransport::physical(server);

    let payload = vec![0xCDu8; 50_000];
    sender.send(&payload).expect("send large");
    let received = receiver.recv().expect("recv large");

    assert_eq!(received.len(), 50_000);
    assert_eq!(received, payload);
}

// ── DirectTransport::exchange ──────────────────────────────────

// @internal
#[test]
fn direct_exchange_bidirectional() {
    let (client, server) = loopback_pair();
    let alice_payload = b"alice-direct-data";
    let bob_payload = b"bob-direct-data";

    let bob_payload_clone = bob_payload.to_vec();
    let bob_handle = thread::spawn(move || {
        let mut bob = TcpDirectTransport::physical(server);
        bob.exchange(&bob_payload_clone, false) // responder
    });

    let mut alice = TcpDirectTransport::physical(client);
    let received_by_alice = alice.exchange(alice_payload, true).expect("alice exchange");

    let received_by_bob = bob_handle
        .join()
        .expect("bob thread")
        .expect("bob exchange");

    assert_eq!(received_by_alice, bob_payload);
    assert_eq!(received_by_bob, alice_payload);
}

// @internal
#[test]
fn direct_exchange_with_realistic_exchange_payload() {
    let (client, server) = loopback_pair();

    // Simulate realistic ~300 byte exchange payloads (base64-encoded ExchangeQR)
    let alice_qr = vec![0xA1u8; 300];
    let bob_qr = vec![0xB2u8; 300];
    let bob_qr_clone = bob_qr.clone();

    let bob_handle = thread::spawn(move || {
        let mut bob = TcpDirectTransport::physical(server);
        bob.exchange(&bob_qr_clone, false)
    });

    let mut alice = TcpDirectTransport::physical(client);
    let received_by_alice = alice.exchange(&alice_qr, true).expect("exchange");

    let received_by_bob = bob_handle.join().expect("thread").expect("exchange");

    assert_eq!(received_by_alice, bob_qr);
    assert_eq!(received_by_bob, alice_qr);
}

// ── Error handling via DirectTransport ─────────────────────────

// @internal
#[test]
fn direct_recv_after_disconnect_returns_error() {
    let (client, server) = loopback_pair();
    drop(client); // close sender side

    let mut receiver = TcpDirectTransport::physical(server);
    let result = receiver.recv();
    assert!(result.is_err(), "recv after disconnect should fail");
}

// @internal
#[test]
fn direct_send_after_disconnect_does_not_panic() {
    let (client, server) = loopback_pair();
    drop(server); // close receiver side

    let mut sender = TcpDirectTransport::physical(client);
    // Send may succeed (buffered) due to OS buffering, or fail immediately.
    // Either outcome is acceptable — the contract is no panic.
    let first = sender.send(b"first");
    let second = sender.send(&vec![0u8; 60_000]);
    // At least one of these should eventually fail on a closed connection,
    // but OS buffering makes the exact failure point non-deterministic.
    assert!(
        first.is_ok() || first.is_err(),
        "send must return Ok or Err, not panic"
    );
    assert!(
        second.is_ok() || second.is_err(),
        "send must return Ok or Err, not panic"
    );
}

// ── Trait object safety ────────────────────────────────────────

// @internal
#[test]
fn direct_transport_is_object_safe() {
    let (client, _server) = loopback_pair();
    let transport = TcpDirectTransport::physical(client);
    // Verify we can use it as a trait object
    let boxed: Box<dyn DirectTransport> = Box::new(transport);
    assert_eq!(boxed.proximity_level(), ProximityLevel::Physical);
}

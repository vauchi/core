// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the TcpDirectTransport frontend utility and ProximityLevel.
//!
//! TcpDirectTransport wraps the VXCH TCP framing protocol for frontends
//! executing `ExchangeCommand::DirectSend` (ADR-031).

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::exchange::direct_transport::ProximityLevel;
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;

fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

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

// @internal
#[test]
fn send_recv_roundtrip() {
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
fn send_recv_large_payload() {
    let (client, server) = loopback_pair();
    let mut sender = TcpDirectTransport::physical(client);
    let mut receiver = TcpDirectTransport::physical(server);

    let payload = vec![0xCDu8; 50_000];
    sender.send(&payload).expect("send large");
    let received = receiver.recv().expect("recv large");

    assert_eq!(received.len(), 50_000);
    assert_eq!(received, payload);
}

// @internal
#[test]
fn exchange_bidirectional() {
    let (client, server) = loopback_pair();
    let alice_payload = b"alice-direct-data";
    let bob_payload = b"bob-direct-data";

    let bob_payload_clone = bob_payload.to_vec();
    let bob_handle = thread::spawn(move || {
        let mut bob = TcpDirectTransport::physical(server);
        bob.exchange(&bob_payload_clone, false)
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
fn recv_after_disconnect_returns_error() {
    let (client, server) = loopback_pair();
    drop(client);

    let mut receiver = TcpDirectTransport::physical(server);
    let result = receiver.recv();
    assert!(result.is_err(), "recv after disconnect should fail");
}

// @internal
#[test]
fn send_empty_payload_is_rejected() {
    let (client, _server) = loopback_pair();
    let mut sender = TcpDirectTransport::physical(client);
    let result = sender.send(b"");
    assert!(
        result.is_err(),
        "empty payload should be rejected on send side"
    );
}

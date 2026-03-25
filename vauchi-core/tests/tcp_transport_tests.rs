// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the TCP exchange transport protocol.
//!
//! Uses loopback TCP connections to verify the protocol without hardware.

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::exchange::tcp_transport::{
    TcpTransportError, exchange_payloads, recv_payload, send_payload,
};

/// Helper: create a connected pair of TCP streams on loopback.
fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

// ── send/recv roundtrip ─────────────────────────────────────

#[test]
fn send_recv_roundtrip() {
    let (mut client, mut server) = loopback_pair();
    let payload = b"VXCH-test-payload-12345";

    send_payload(&mut client, payload).expect("send");
    let received = recv_payload(&mut server).expect("recv");

    assert_eq!(received, payload);
}

#[test]
fn send_recv_empty_payload_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    // Sending empty payload: length=0 is valid on send side
    send_payload(&mut client, b"").expect("send empty");
    // But recv treats length=0 as ConnectionClosed
    let err = recv_payload(&mut server).expect_err("should fail");
    assert!(matches!(err, TcpTransportError::ConnectionClosed));
}

#[test]
fn send_recv_large_payload() {
    let (mut client, mut server) = loopback_pair();
    let payload = vec![0xABu8; 60_000]; // Under 64KB limit

    send_payload(&mut client, &payload).expect("send large");
    let received = recv_payload(&mut server).expect("recv large");

    assert_eq!(received.len(), 60_000);
    assert_eq!(received, payload);
}

// ── bidirectional exchange ──────────────────────────────────

#[test]
fn exchange_payloads_bidirectional() {
    let (mut client, mut server) = loopback_pair();
    let alice_payload = b"alice-exchange-data";
    let bob_payload = b"bob-exchange-data";

    let bob_handle = thread::spawn(move || {
        exchange_payloads(&mut server, bob_payload, false) // responder
    });

    let received_by_alice =
        exchange_payloads(&mut client, alice_payload, true).expect("alice exchange");

    let received_by_bob = bob_handle
        .join()
        .expect("bob thread")
        .expect("bob exchange");

    assert_eq!(received_by_alice, bob_payload);
    assert_eq!(received_by_bob, alice_payload);
}

#[test]
fn exchange_with_realistic_qr_payload() {
    let (mut client, mut server) = loopback_pair();

    // Simulate a realistic exchange payload (~300 bytes base64)
    let alice_qr = "VlhDSA==".repeat(40); // ~320 bytes
    let bob_qr = "Qk9CUVI=".repeat(40);
    let bob_qr_expected = bob_qr.clone();

    let bob_handle =
        thread::spawn(move || exchange_payloads(&mut server, bob_qr.as_bytes(), false));

    let received = exchange_payloads(&mut client, alice_qr.as_bytes(), true).expect("exchange");
    let bob_received = bob_handle.join().expect("thread").expect("exchange");

    assert_eq!(received, bob_qr_expected.as_bytes());
    assert_eq!(bob_received, alice_qr.as_bytes());
}

// ── error handling ──────────────────────────────────────────

#[test]
fn recv_invalid_magic_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    // Send garbage instead of protocol magic
    use std::io::Write;
    client.write_all(b"XXXX").expect("write garbage");
    client.write_all(&[1]).expect("write version");
    client.write_all(&4u32.to_be_bytes()).expect("write len");
    client.write_all(b"test").expect("write payload");

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::InvalidMagic));
}

#[test]
fn recv_wrong_version_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[99]).expect("bad version");
    client.write_all(&4u32.to_be_bytes()).expect("len");
    client.write_all(b"test").expect("payload");

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::UnsupportedVersion(99)));
}

#[test]
fn recv_oversized_length_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[1]).expect("version");
    client
        .write_all(&100_000u32.to_be_bytes())
        .expect("huge len");
    // Don't send actual data — the length check should fail first

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(100_000)));
}

#[test]
fn send_oversized_payload_is_rejected() {
    let (mut client, _server) = loopback_pair();
    let huge = vec![0u8; 100_000];

    let err = send_payload(&mut client, &huge).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(100_000)));
}

#[test]
fn recv_connection_closed_mid_payload() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[1]).expect("version");
    client.write_all(&100u32.to_be_bytes()).expect("len=100");
    client.write_all(&[0u8; 10]).expect("partial data");
    drop(client); // Close connection mid-payload

    let err = recv_payload(&mut server).expect_err("should fail");
    assert!(matches!(err, TcpTransportError::Io(_)));
}

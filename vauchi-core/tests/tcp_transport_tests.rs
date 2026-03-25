// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the TCP exchange transport protocol.
//!
//! Uses loopback TCP connections to verify the protocol without hardware.

use std::net::{TcpListener, TcpStream};
use std::thread;

use vauchi_core::exchange::{TcpTransportError, exchange_payloads, recv_payload, send_payload};

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
fn send_empty_payload_is_rejected() {
    let (mut client, _server) = loopback_pair();

    let err = send_payload(&mut client, b"").expect_err("should reject empty");
    assert!(matches!(err, TcpTransportError::EmptyPayload));
}

#[test]
fn recv_zero_length_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    // Manually craft a frame with length=0
    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[1]).expect("version");
    client.write_all(&0u32.to_be_bytes()).expect("len=0");

    let err = recv_payload(&mut server).expect_err("should reject zero length");
    assert!(matches!(err, TcpTransportError::EmptyPayload));
}

#[test]
fn send_recv_max_size_payload() {
    let (mut client, mut server) = loopback_pair();
    let payload = vec![0xABu8; 4_096]; // Exactly MAX_PAYLOAD_SIZE

    send_payload(&mut client, &payload).expect("send max");
    let received = recv_payload(&mut server).expect("recv max");

    assert_eq!(received.len(), 4_096);
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

    // Simulate a realistic exchange payload (~320 bytes base64)
    let alice_qr = "VlhDSA==".repeat(40);
    let bob_qr = "Qk9CUVI=".repeat(40);
    let bob_qr_expected = bob_qr.clone();

    let bob_handle =
        thread::spawn(move || exchange_payloads(&mut server, bob_qr.as_bytes(), false));

    let received = exchange_payloads(&mut client, alice_qr.as_bytes(), true).expect("exchange");
    let bob_received = bob_handle.join().expect("thread").expect("exchange");

    assert_eq!(received, bob_qr_expected.as_bytes());
    assert_eq!(bob_received, alice_qr.as_bytes());
}

#[test]
fn exchange_initiator_fails_on_recv_when_peer_drops() {
    let (mut client, server) = loopback_pair();
    drop(server); // Peer gone before initiator sends

    let err = exchange_payloads(&mut client, b"payload", true).expect_err("should fail");
    assert!(matches!(err, TcpTransportError::Io(_)));
}

#[test]
fn exchange_responder_fails_on_recv_when_peer_drops() {
    let (client, mut server) = loopback_pair();
    drop(client); // Peer gone before responder receives

    let err = exchange_payloads(&mut server, b"payload", false).expect_err("should fail");
    assert!(matches!(err, TcpTransportError::Io(_)));
}

// ── error handling ──────────────────────────────────────────

#[test]
fn recv_invalid_magic_is_rejected() {
    let (mut client, mut server) = loopback_pair();

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

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(100_000)));
}

#[test]
fn send_oversized_payload_is_rejected() {
    let (mut client, _server) = loopback_pair();
    let huge = vec![0u8; 5_000]; // Over 4KB limit

    let err = send_payload(&mut client, &huge).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(5_000)));
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

// ── CC-14 adversarial inputs ────────────────────────────────

#[test]
fn recv_all_null_bytes_payload() {
    let (mut client, mut server) = loopback_pair();
    let payload = vec![0u8; 100]; // 100 null bytes

    send_payload(&mut client, &payload).expect("send nulls");
    let received = recv_payload(&mut server).expect("recv nulls");
    assert_eq!(received, payload);
}

#[test]
fn recv_boundary_max_plus_one_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[1]).expect("version");
    client
        .write_all(&4_097u32.to_be_bytes()) // MAX_PAYLOAD_SIZE + 1
        .expect("len");

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(4_097)));
}

#[test]
fn recv_u32_max_length_is_rejected() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VXCH").expect("magic");
    client.write_all(&[1]).expect("version");
    client
        .write_all(&u32::MAX.to_be_bytes())
        .expect("max u32 len");

    let err = recv_payload(&mut server).expect_err("should reject");
    assert!(matches!(err, TcpTransportError::PayloadTooLarge(u32::MAX)));
}

#[test]
fn recv_truncated_header_is_io_error() {
    let (mut client, mut server) = loopback_pair();

    use std::io::Write;
    client.write_all(b"VX").expect("partial magic");
    drop(client); // Close mid-header

    let err = recv_payload(&mut server).expect_err("should fail");
    assert!(matches!(err, TcpTransportError::Io(_)));
}

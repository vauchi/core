// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for device linking over direct transport (USB/TCP).
//!
//! Verifies the full device linking protocol over `TcpDirectTransport`:
//! QR transfer → request → confirmation → response → master seed received.

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use vauchi_core::exchange::DirectTransport;
use vauchi_core::exchange::device_link::{
    DeviceLinkInitiator, DeviceLinkResponder, ProximityProof, recv_device_link_qr,
    recv_encrypted_blob, send_device_link_qr, send_encrypted_blob,
};
use vauchi_core::exchange::tcp_transport::TcpDirectTransport;
use vauchi_core::identity::{DeviceRegistry, Identity};

fn loopback_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).expect("connect");
    let (server, _) = listener.accept().expect("accept");
    (client, server)
}

fn create_test_identity() -> Identity {
    Identity::create("Test User")
}

fn create_test_registry(identity: &Identity) -> DeviceRegistry {
    let device_info = identity.device_info();
    let master_seed = [0x42u8; 32];
    DeviceRegistry::new(
        device_info.to_registered(&master_seed),
        identity.signing_keypair(),
    )
}

fn create_manual_proof(initiator: &DeviceLinkInitiator, confirmation_code: &str) -> ProximityProof {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mac = vauchi_core::exchange::device_link::compute_confirmation_mac(
        initiator.qr().link_key(),
        confirmation_code,
    );
    ProximityProof::ManualConfirmation {
        confirmation_code_mac: mac,
        confirmed_at: now,
    }
}

// ── Full device link over TCP ──────────────────────────────────

// @internal
#[test]
fn full_device_link_over_tcp_transport() {
    let identity = create_test_identity();
    let registry = create_test_registry(&identity);
    let master_seed = [0x42u8; 32];

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let (client, server) = loopback_pair();

    // Initiator side (existing device): sends QR, receives request, sends response
    let initiator_handle = thread::spawn(move || {
        let mut transport = TcpDirectTransport::physical(client);

        // Round 1: Send DeviceLinkQR
        send_device_link_qr(&mut transport, initiator.qr()).expect("send qr");

        // Round 2: Receive encrypted request
        let encrypted_request = recv_encrypted_blob(&mut transport).expect("recv request");

        // Prepare confirmation and verify
        let (confirmation, request) = initiator
            .prepare_confirmation(&encrypted_request)
            .expect("prepare confirmation");

        assert_eq!(confirmation.device_name, "New Desktop");
        assert!(!confirmation.confirmation_code.is_empty());

        // Create proximity proof (manual confirmation for USB)
        let proof = create_manual_proof(&initiator, &confirmation.confirmation_code);

        // Confirm link and get response
        let (encrypted_response, updated_registry, new_device) = initiator
            .confirm_link(&request, &proof)
            .expect("confirm link");

        // Round 3: Send encrypted response
        send_encrypted_blob(&mut transport, &encrypted_response).expect("send response");

        (updated_registry, new_device)
    });

    // Responder side (new device): receives QR, sends request, receives response
    let mut responder_transport = TcpDirectTransport::physical(server);

    // Round 1: Receive DeviceLinkQR
    let qr = recv_device_link_qr(&mut responder_transport).expect("recv qr");
    assert!(!qr.is_expired());
    assert!(qr.verify_signature());

    // Create responder from received QR
    let mut responder =
        DeviceLinkResponder::from_qr(qr, "New Desktop".to_string()).expect("from_qr");

    // Round 2: Create and send encrypted request
    let encrypted_request = responder.create_request().expect("create request");
    send_encrypted_blob(&mut responder_transport, &encrypted_request).expect("send request");

    // Verify confirmation code matches
    let responder_code = responder
        .compute_confirmation_code()
        .expect("confirmation code");
    assert!(!responder_code.is_empty());

    // Round 3: Receive encrypted response
    let encrypted_response = recv_encrypted_blob(&mut responder_transport).expect("recv response");
    let response = responder
        .process_response(&encrypted_response)
        .expect("process response");

    // Verify the responder received a valid master seed
    assert_eq!(response.master_seed(), &[0x42u8; 32]);
    assert!(!response.display_name().is_empty());

    // Verify initiator side completed successfully
    let (updated_registry, new_device) = initiator_handle.join().expect("initiator thread");
    assert_eq!(new_device.device_name(), "New Desktop");
    assert!(
        updated_registry.active_devices().len() >= 2,
        "Registry should have at least 2 devices after linking"
    );
}

// ── Error cases ────────────────────────────────────────────────

// @internal
#[test]
fn recv_device_link_qr_rejects_invalid_data() {
    let (client, server) = loopback_pair();

    thread::spawn(move || {
        let mut transport = TcpDirectTransport::physical(client);
        transport.send(b"not-a-valid-device-link-qr").expect("send");
    });

    let mut transport = TcpDirectTransport::physical(server);
    let result = recv_device_link_qr(&mut transport);
    assert!(result.is_err(), "Invalid QR data should be rejected");
}

// @internal
#[test]
fn recv_device_link_qr_rejects_after_disconnect() {
    let (client, server) = loopback_pair();
    drop(client);

    let mut transport = TcpDirectTransport::physical(server);
    let result = recv_device_link_qr(&mut transport);
    assert!(result.is_err(), "Should fail on disconnected transport");
}

// @internal
#[test]
fn send_recv_encrypted_blob_roundtrip() {
    let (client, server) = loopback_pair();
    let blob = vec![0xABu8; 256];
    let expected = blob.clone();

    let handle = thread::spawn(move || {
        let mut transport = TcpDirectTransport::physical(client);
        send_encrypted_blob(&mut transport, &blob).expect("send");
    });

    let mut transport = TcpDirectTransport::physical(server);
    let received = recv_encrypted_blob(&mut transport).expect("recv");

    assert_eq!(received, expected);
    handle.join().expect("sender thread");
}

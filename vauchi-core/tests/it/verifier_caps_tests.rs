// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for proximity verifier capability bits in TransportCaps.
//!
//! AMBIENT_AUDIO (bit 6) and ACCELEROMETER (bit 7) indicate that a device
//! supports passive room-noise fingerprinting or tap-pairing respectively.
//! They are advertised alongside transport caps in the QR/NFC payload.

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::TransportCaps;

// ===== Bit values =====

// @internal
#[test]
fn ambient_audio_bit_is_0x40() {
    assert_eq!(TransportCaps::AMBIENT_AUDIO.bits(), 0x0040);
}

// @internal
#[test]
fn accelerometer_bit_is_0x80() {
    assert_eq!(TransportCaps::ACCELEROMETER.bits(), 0x0080);
}

// ===== No overlap with existing transport bits =====

// @internal
#[test]
fn verifier_bits_do_not_overlap_transport_bits() {
    let transport_bits = TransportCaps::STATIC_QR
        | TransportCaps::ANIMATED_QR
        | TransportCaps::BLE
        | TransportCaps::WIFI_AWARE
        | TransportCaps::NFC_TRIGGER
        | TransportCaps::TCP;
    let verifier_bits = TransportCaps::AMBIENT_AUDIO | TransportCaps::ACCELEROMETER;
    assert!(
        (transport_bits & verifier_bits).is_empty(),
        "Verifier bits must not overlap transport bits"
    );
}

// ===== Wire-format roundtrip =====

// @internal
#[test]
fn ambient_audio_roundtrip() {
    let caps = TransportCaps::STATIC_QR | TransportCaps::AMBIENT_AUDIO;
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
    assert!(restored.contains(TransportCaps::AMBIENT_AUDIO));
    assert!(restored.contains(TransportCaps::STATIC_QR));
}

// @internal
#[test]
fn accelerometer_roundtrip() {
    let caps = TransportCaps::BLE | TransportCaps::ACCELEROMETER;
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
    assert!(restored.contains(TransportCaps::ACCELEROMETER));
    assert!(restored.contains(TransportCaps::BLE));
}

// @internal
#[test]
fn both_verifier_caps_roundtrip() {
    let caps = TransportCaps::STATIC_QR
        | TransportCaps::BLE
        | TransportCaps::AMBIENT_AUDIO
        | TransportCaps::ACCELEROMETER;
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
}

// ===== Backward compatibility =====

// @internal
#[test]
fn v2_peer_without_verifier_caps_still_works() {
    // v2 peer only advertises transports, no verifier bits
    let v2_caps = TransportCaps::STATIC_QR | TransportCaps::BLE;
    assert!(!v2_caps.contains(TransportCaps::AMBIENT_AUDIO));
    assert!(!v2_caps.contains(TransportCaps::ACCELEROMETER));
}

// @internal
#[test]
fn verifier_caps_do_not_affect_transport_negotiation() {
    use vauchi_core::exchange::transport::negotiation::negotiate_transport;

    let ours = TransportCaps::BLE | TransportCaps::STATIC_QR;
    let ours_with_verifiers = ours | TransportCaps::AMBIENT_AUDIO | TransportCaps::ACCELEROMETER;
    let theirs = TransportCaps::BLE | TransportCaps::STATIC_QR;

    assert_eq!(
        negotiate_transport(&ours, &theirs),
        negotiate_transport(&ours_with_verifiers, &theirs),
        "Verifier caps must not influence transport selection"
    );
}

// ===== Intersection for verifier negotiation =====

// @internal
#[test]
fn both_peers_have_ambient_audio() {
    let ours = TransportCaps::STATIC_QR | TransportCaps::AMBIENT_AUDIO;
    let theirs = TransportCaps::STATIC_QR | TransportCaps::AMBIENT_AUDIO;
    let common = ours & theirs;
    assert!(common.contains(TransportCaps::AMBIENT_AUDIO));
}

// @internal
#[test]
fn only_one_peer_has_accelerometer() {
    let ours = TransportCaps::STATIC_QR | TransportCaps::ACCELEROMETER;
    let theirs = TransportCaps::STATIC_QR;
    let common = ours & theirs;
    assert!(
        !common.contains(TransportCaps::ACCELEROMETER),
        "Accelerometer requires both peers"
    );
}

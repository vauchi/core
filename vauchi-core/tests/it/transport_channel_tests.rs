// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::{TransportCaps, TransportType};

// @internal
#[test]
fn test_transport_type_as_str() {
    assert_eq!(TransportType::WifiAware.as_str(), "wifi_aware");
    assert_eq!(TransportType::Ble.as_str(), "ble");
    assert_eq!(TransportType::AnimatedQr.as_str(), "animated_qr");
    assert_eq!(TransportType::StaticQr.as_str(), "static_qr");
    assert_eq!(TransportType::Nfc.as_str(), "nfc");
    assert_eq!(TransportType::Tcp.as_str(), "tcp");
}

// @internal
#[test]
fn test_transport_type_display() {
    assert_eq!(format!("{}", TransportType::WifiAware), "wifi_aware");
    assert_eq!(format!("{}", TransportType::Ble), "ble");
}

// @internal
#[test]
fn test_transport_priority_ordering() {
    assert!(TransportType::WifiAware.priority() > TransportType::Tcp.priority());
    assert!(TransportType::Tcp.priority() > TransportType::Ble.priority());
    assert!(TransportType::Nfc.priority() > TransportType::Ble.priority());
    assert!(TransportType::Ble.priority() > TransportType::AnimatedQr.priority());
    assert!(TransportType::AnimatedQr.priority() > TransportType::StaticQr.priority());
}

// @internal
#[test]
fn test_transport_caps_bitfield_operations() {
    let caps = TransportCaps::STATIC_QR | TransportCaps::BLE;
    assert!(caps.contains(TransportCaps::STATIC_QR));
    assert!(caps.contains(TransportCaps::BLE));
    assert!(!caps.contains(TransportCaps::WIFI_AWARE));
    assert!(!caps.contains(TransportCaps::ANIMATED_QR));
}

// @internal
#[test]
fn test_transport_caps_serialize_roundtrip() {
    let caps = TransportCaps::STATIC_QR | TransportCaps::ANIMATED_QR | TransportCaps::WIFI_AWARE;
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
}

// @internal
#[test]
fn test_transport_caps_all_flags_roundtrip() {
    let caps = TransportCaps::all();
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
}

// @internal
#[test]
fn test_transport_caps_empty_roundtrip() {
    let caps = TransportCaps::empty();
    let bytes = caps.to_bytes();
    let restored = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, restored);
    assert!(restored.is_empty());
}

// @internal
#[test]
fn test_transport_caps_v2_backward_compat() {
    // v2 peers send 0 caps — from_bytes should produce empty
    let caps = TransportCaps::from_bytes([0, 0]);
    assert!(caps.is_empty());
    // Default to STATIC_QR in application logic
    let with_default = caps | TransportCaps::STATIC_QR;
    assert!(with_default.contains(TransportCaps::STATIC_QR));
}

// @internal
#[test]
fn test_transport_caps_unknown_bits_truncated() {
    // Future flags (bits 6-15) should be silently ignored
    let bytes = [0xFF, 0xFF];
    let caps = TransportCaps::from_bytes(bytes);
    assert_eq!(caps, TransportCaps::all());
}

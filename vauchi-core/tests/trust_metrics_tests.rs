// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trust Metrics Tests
//!
//! Tests for contact enrichment: transport persistence, recovery flag,
//! card freshness, and storage roundtrip.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::exchange::ExchangeTransport;

#[test]
fn test_exchange_transport_serde_roundtrip_qr() {
    let transport = ExchangeTransport::Qr;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Qr\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Qr);
}

#[test]
fn test_exchange_transport_serde_roundtrip_nfc() {
    let transport = ExchangeTransport::Nfc;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Nfc\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Nfc);
}

#[test]
fn test_exchange_transport_serde_roundtrip_ble() {
    let transport = ExchangeTransport::Ble;
    let json = serde_json::to_string(&transport).unwrap();
    assert_eq!(json, "\"Ble\"");
    let deserialized: ExchangeTransport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ExchangeTransport::Ble);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::negotiation::negotiate_transport;
use vauchi_core::exchange::transport::{TransportCaps, TransportType};

// @internal
#[test]
fn both_have_wifi_aware_selects_wifi_aware() {
    let ours = TransportCaps::WIFI_AWARE | TransportCaps::BLE | TransportCaps::STATIC_QR;
    let theirs = TransportCaps::WIFI_AWARE | TransportCaps::STATIC_QR;
    assert_eq!(
        negotiate_transport(&ours, &theirs),
        TransportType::WifiAware
    );
}

// @internal
#[test]
fn no_wifi_aware_both_have_ble_selects_ble() {
    let ours = TransportCaps::BLE | TransportCaps::STATIC_QR;
    let theirs = TransportCaps::BLE | TransportCaps::ANIMATED_QR | TransportCaps::STATIC_QR;
    assert_eq!(negotiate_transport(&ours, &theirs), TransportType::Ble);
}

// @internal
#[test]
fn both_have_animated_qr_only_selects_animated_qr() {
    let ours = TransportCaps::ANIMATED_QR;
    let theirs = TransportCaps::ANIMATED_QR;
    assert_eq!(
        negotiate_transport(&ours, &theirs),
        TransportType::AnimatedQr
    );
}

// @internal
#[test]
fn both_have_static_qr_only_selects_static_qr() {
    let ours = TransportCaps::STATIC_QR;
    let theirs = TransportCaps::STATIC_QR;
    assert_eq!(negotiate_transport(&ours, &theirs), TransportType::StaticQr);
}

// @internal
#[test]
fn no_overlap_falls_back_to_static_qr() {
    let ours = TransportCaps::BLE;
    let theirs = TransportCaps::WIFI_AWARE;
    assert_eq!(negotiate_transport(&ours, &theirs), TransportType::StaticQr);
}

// @internal
#[test]
fn all_flags_selects_wifi_aware_highest_priority() {
    let all = TransportCaps::STATIC_QR
        | TransportCaps::ANIMATED_QR
        | TransportCaps::BLE
        | TransportCaps::WIFI_AWARE
        | TransportCaps::NFC_TRIGGER
        | TransportCaps::TCP;
    assert_eq!(negotiate_transport(&all, &all), TransportType::WifiAware);
}

// @internal
#[test]
fn v2_peer_with_empty_caps_falls_back_to_static_qr() {
    let ours = TransportCaps::WIFI_AWARE | TransportCaps::BLE | TransportCaps::STATIC_QR;
    let theirs = TransportCaps::empty();
    assert_eq!(negotiate_transport(&ours, &theirs), TransportType::StaticQr);
}

// @internal
#[test]
fn negotiation_is_symmetric() {
    let a = TransportCaps::TCP | TransportCaps::BLE | TransportCaps::STATIC_QR;
    let b = TransportCaps::BLE | TransportCaps::ANIMATED_QR;
    assert_eq!(negotiate_transport(&a, &b), negotiate_transport(&b, &a));
}

// @internal
#[test]
fn tcp_preferred_over_ble() {
    let ours = TransportCaps::TCP | TransportCaps::BLE;
    let theirs = TransportCaps::TCP | TransportCaps::BLE;
    assert_eq!(negotiate_transport(&ours, &theirs), TransportType::Tcp);
}

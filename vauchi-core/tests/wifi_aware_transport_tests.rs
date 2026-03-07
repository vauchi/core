// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use std::time::Duration;
use vauchi_core::exchange::transport::wifi_aware::{
    MockWifiAwareBackend, WifiAwareConfig, WifiAwareTransport,
};
use vauchi_core::exchange::transport::{TransportChannel, TransportType};

#[test]
fn transport_type_returns_wifi_aware() {
    let backend = MockWifiAwareBackend::new();
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    assert_eq!(transport.transport_type(), TransportType::WifiAware);
}

#[test]
fn available_when_backend_supports_it() {
    let backend = MockWifiAwareBackend::new().with_available(true);
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    let available = transport
        .is_available()
        .expect("is_available should not fail");
    assert!(
        available,
        "transport should be available when backend supports it"
    );
}

#[test]
fn unavailable_when_backend_does_not_support_it() {
    let backend = MockWifiAwareBackend::new().with_available(false);
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    let available = transport
        .is_available()
        .expect("is_available should not fail");
    assert!(
        !available,
        "transport should be unavailable when backend does not support it"
    );
}

#[test]
fn send_receive_roundtrip_with_mock() {
    let payload = b"hello vauchi".to_vec();
    let backend = MockWifiAwareBackend::new()
        .with_available(true)
        .with_peer("peer-1")
        .queue_receive(payload.clone());
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());

    transport.send(&payload).expect("send should succeed");
    let received = transport
        .receive(Duration::from_secs(5))
        .expect("receive should succeed");
    assert_eq!(received, payload, "received data must match sent data");
}

#[test]
fn discover_peer_finds_mock_peer() {
    let backend = MockWifiAwareBackend::new()
        .with_available(true)
        .with_peer("wifi-peer-42");
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());

    let peer = transport
        .discover_peer(Duration::from_secs(5))
        .expect("discover_peer should find mock peer");
    assert_eq!(peer.peer_id, "wifi-peer-42");
}

#[test]
fn no_chunking_needed_max_payload_65536() {
    let backend = MockWifiAwareBackend::new();
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    assert_eq!(transport.max_payload_size(), 65536);
    assert!(
        !transport.requires_chunking(),
        "WiFi Aware should not require chunking"
    );
}

#[test]
fn discover_peer_timeout_when_no_peers() {
    let backend = MockWifiAwareBackend::new().with_available(true);
    // No peers added — discover should timeout
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    let result = transport.discover_peer(Duration::from_millis(100));
    assert!(result.is_err(), "discover_peer should fail when no peers");
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("timed out"),
        "error should mention timeout, got: {}",
        err_msg
    );
}

#[test]
fn default_config_has_expected_values() {
    let config = WifiAwareConfig::default();
    assert_eq!(config.service_name, "vauchi-exchange");
    assert_eq!(config.timeout, Duration::from_secs(10));
}

#[test]
fn close_succeeds() {
    let backend = MockWifiAwareBackend::new().with_available(true);
    let transport = WifiAwareTransport::new(backend, WifiAwareConfig::default());
    transport.close().expect("close should succeed");
}

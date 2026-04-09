// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for api::config
//! Extracted from config.rs

use std::path::PathBuf;
use vauchi_core::api::*;
use vauchi_core::network::PinnedCertificate;

#[test]
fn test_vauchi_config_default() {
    let config = VauchiConfig::default();

    assert_eq!(config.storage_path, PathBuf::from("./vauchi_data"));
    assert!(config.auto_save);
    assert_eq!(
        config.relay.server_url, "https://relay.vauchi.app",
        "Default relay URL must be the production relay"
    );
}

#[test]
fn test_default_relay_url_is_valid_https() {
    let config = VauchiConfig::default();
    assert!(
        config.relay.server_url.starts_with("https://"),
        "Default relay URL must use https:// scheme"
    );
    assert!(
        !config.relay.server_url.is_empty(),
        "Default relay URL must not be empty"
    );
}

#[test]
fn test_vauchi_config_builder() {
    let config = VauchiConfig::with_storage_path("/tmp/test")
        .with_relay_url("https://relay.example.com")
        .without_auto_save();

    assert_eq!(config.storage_path, PathBuf::from("/tmp/test"));
    assert_eq!(config.relay.server_url, "https://relay.example.com");
    assert!(!config.auto_save);
}

#[test]
fn test_relay_config_default() {
    let config = RelayConfig::default();

    assert_eq!(config.server_url, "https://relay.vauchi.app");
    assert_eq!(config.connect_timeout_ms, 10_000);
    assert_eq!(config.io_timeout_ms, 30_000);
    assert_eq!(config.max_reconnect_attempts, 5);
    assert_eq!(config.max_pending_messages, 100);
    assert_eq!(config.ack_timeout_ms, 30_000);
}

#[test]
fn test_relay_config_to_transport_config() {
    let relay = RelayConfig {
        server_url: "https://test.com".into(),
        connect_timeout_ms: 5_000,
        io_timeout_ms: 15_000,
        max_reconnect_attempts: 3,
        reconnect_base_delay_ms: 500,
        ..Default::default()
    };

    let transport = relay.to_transport_config();

    assert_eq!(transport.server_url, "https://test.com");
    assert_eq!(transport.connect_timeout_ms, 5_000);
    assert_eq!(transport.io_timeout_ms, 15_000);
    assert_eq!(transport.max_reconnect_attempts, 3);
    assert_eq!(transport.reconnect_base_delay_ms, 500);
}

#[test]
fn test_relay_config_to_relay_client_config() {
    let relay = RelayConfig {
        server_url: "https://test.com".into(),
        max_pending_messages: 50,
        ack_timeout_ms: 15_000,
        max_retries: 3,
        ..Default::default()
    };

    let client_config = relay.to_relay_client_config(true, false);

    assert_eq!(client_config.transport.server_url, "https://test.com");
    assert_eq!(client_config.max_pending_messages, 50);
    assert_eq!(client_config.ack_timeout_ms, 15_000);
    assert_eq!(client_config.max_retries, 3);
}

#[test]
fn test_sync_config_default() {
    let config = SyncConfig::default();

    assert!(config.auto_sync);
    assert_eq!(config.sync_interval_ms, 60_000);
    assert_eq!(config.max_pending_updates, 50);
}

// ─── Certificate pinning defaults ───────────────────────────────────

/// relay.vauchi.app SPKI SHA-256 fingerprint.
/// Extracted via: openssl s_client → x509 pubkey → pkey DER → sha256
const RELAY_SPKI_SHA256: [u8; 32] = [
    0xba, 0xae, 0x88, 0x27, 0xcb, 0xce, 0xf3, 0xe5, 0xa1, 0xcc, 0xe3, 0xe0, 0x00, 0x9d, 0x4e, 0x06,
    0xe1, 0x70, 0x0f, 0xb1, 0x00, 0xeb, 0x37, 0x84, 0xb8, 0xc3, 0x4f, 0x4e, 0x26, 0xb0, 0x6d, 0x00,
];

/// @internal C7
#[test]
fn default_relay_config_has_production_pin() {
    let config = RelayConfig::default();
    let expected = PinnedCertificate::new(RELAY_SPKI_SHA256);

    assert_eq!(
        config.pinned_certs.len(),
        1,
        "Default relay config must include exactly one pinned certificate"
    );
    assert_eq!(
        config.pinned_certs[0], expected,
        "Default pin must match relay.vauchi.app SPKI SHA-256"
    );
}

/// @internal C7
#[test]
fn relay_config_pin_propagates_to_transport_config() {
    let relay = RelayConfig::default();
    let transport = relay.to_transport_config();

    assert_eq!(
        transport.pinned_certs.len(),
        relay.pinned_certs.len(),
        "Transport config must carry same pin count as relay config"
    );
    assert_eq!(
        transport.pinned_certs, relay.pinned_certs,
        "Transport config pins must match relay config pins"
    );
}

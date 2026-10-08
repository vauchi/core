// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for api::config
//! Extracted from config.rs

use std::path::PathBuf;
use vauchi_core::api::*;

// @internal
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

// @internal
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

// @internal
#[test]
fn test_vauchi_config_builder() {
    let config = VauchiConfig::with_storage_path("/tmp/test")
        .with_relay_url("https://relay.example.com")
        .without_auto_save();

    assert_eq!(config.storage_path, PathBuf::from("/tmp/test"));
    assert_eq!(config.relay.server_url, "https://relay.example.com");
    assert!(!config.auto_save);
}

// @internal
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

// @internal
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

// @internal
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

// @internal
#[test]
fn test_sync_config_default() {
    let config = SyncConfig::default();

    assert!(config.auto_sync);
    assert_eq!(config.sync_interval_ms, 60_000);
    assert_eq!(config.max_pending_updates, 50);
}

// ─── Certificate pinning defaults ───────────────────────────────────

/// @internal C7
// @internal
#[test]
fn default_relay_config_has_production_pin() {
    let config = RelayConfig::default();
    let default_pins = RelayConfig::default_pins();

    assert_eq!(
        config.pinned_certs.len(),
        1,
        "Default relay config must include exactly one pinned certificate"
    );
    assert_eq!(
        config.pinned_certs, default_pins,
        "Default pins must match default_pins() — single source of truth"
    );
}

/// @internal C7
// @internal
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

/// @internal C7
// @internal
#[test]
fn default_relay_config_has_no_pin_rotation_key() {
    let config = RelayConfig::default();
    assert!(
        config.pin_config_verify_key.is_none(),
        "Pin rotation must be disabled by default (no verify key)"
    );
}

/// @internal C7
// @internal
#[test]
fn default_relay_config_has_24h_pin_ttl() {
    let config = RelayConfig::default();
    assert_eq!(
        config.pin_ttl_secs, 86_400,
        "Default pin TTL must be 24 hours"
    );
}

// ── OHTTP trust anchor (#288, decisions 0.6 and 0.9) ────────────────

const ANCHOR: [u8; 32] = [0x5a; 32];
const OTHER_ANCHOR: [u8; 32] = [0xa5; 32];

/// A relay's anchor travels with its URL (0.9): the gateway key of that
/// relay is accepted only under that anchor.
// @internal
#[test]
fn a_relay_set_with_its_anchor_resolves_to_that_anchor() {
    let config = VauchiConfig::default().with_relay("https://relay.self.example", ANCHOR);

    assert_eq!(config.relay.server_url, "https://relay.self.example");
    assert_eq!(config.relay.ohttp_trust_anchor(), Some(ANCHOR));
}

/// Without an anchor a custom relay has none, and its gateway key cannot be
/// accepted — never one borrowed from elsewhere.
// @internal
#[test]
fn a_custom_relay_without_an_anchor_has_none() {
    let config = VauchiConfig::default().with_relay_url("https://relay.self.example");

    assert_eq!(config.relay.ohttp_trust_anchor(), None);
}

/// Switching relays must not carry the previous relay's anchor over to the
/// new one: that would make the new relay's keys verify against a key its
/// operator never held.
// @internal
#[test]
fn changing_the_relay_url_drops_the_previous_relays_anchor() {
    let config = VauchiConfig::default()
        .with_relay("https://relay.one.example", ANCHOR)
        .with_relay_url("https://relay.two.example");

    assert_eq!(config.relay.ohttp_trust_anchor(), None);
}

// @internal
#[test]
fn setting_a_relay_again_replaces_its_anchor() {
    let config = VauchiConfig::default()
        .with_relay("https://relay.one.example", ANCHOR)
        .with_relay("https://relay.two.example", OTHER_ANCHOR);

    assert_eq!(config.relay.ohttp_trust_anchor(), Some(OTHER_ANCHOR));
}

/// Only the exact production host may fall back to Vauchi's anchor; a
/// lookalike host is a custom relay like any other.
// @internal
#[test]
fn a_lookalike_of_the_production_host_gets_no_anchor() {
    for url in [
        "https://relay.vauchi.app.evil.example",
        "https://evil.example/relay.vauchi.app",
        "https://xrelay.vauchi.app",
    ] {
        let config = VauchiConfig::default().with_relay_url(url);

        assert_eq!(config.relay.ohttp_trust_anchor(), None, "{url}");
    }
}

/// An explicit anchor also wins for the production host: e2e and local
/// stacks point a production-shaped URL at a test gateway.
// @internal
#[test]
fn an_explicit_anchor_wins_for_the_production_host_too() {
    let config = VauchiConfig::default().with_relay("https://relay.vauchi.app", ANCHOR);

    assert_eq!(config.relay.ohttp_trust_anchor(), Some(ANCHOR));
}

/// Vauchi's OHTTP trust anchor and the commitment to its backup, created
/// in the first anchor ceremony (2026-10-08, runbook
/// 2026-10-06-ohttp-anchor-ceremony §2). The production relay's signed key
/// record verifies under this anchor; a client released without them would
/// take no signed key from production, and without the commitment could
/// not follow a rollover.
// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn the_production_relay_carries_vauchis_anchor_and_backup_commitment() {
    let config = VauchiConfig::default();

    assert_eq!(
        config
            .relay
            .ohttp_trust_anchor()
            .map(hex::encode)
            .as_deref(),
        Some("b049502a53371a91a4e29c32dec08d2fb62ebfb17f23fece3306cdef755b1abe")
    );
    assert_eq!(
        config
            .relay
            .ohttp_backup_commitment()
            .map(hex::encode)
            .as_deref(),
        Some("47cb7feaec43837b15fd2248e5c6423f7c8ffd28ed1f0fc0f77d8f6993e9ef45")
    );
}

/// A lookalike host gets neither: the production commitment belongs to the
/// production anchor only.
// @internal
#[test]
fn a_lookalike_of_the_production_host_gets_no_backup_commitment() {
    let config = VauchiConfig::default().with_relay_url("https://relay.vauchi.app.evil.example");

    assert_eq!(config.relay.ohttp_backup_commitment(), None);
}

// @internal
#[test]
fn the_default_relay_config_names_no_explicit_anchor() {
    assert_eq!(RelayConfig::default().ohttp_anchor, None);
    assert_eq!(
        RelayConfig::unpinned("https://relay.self.example".into()).ohttp_anchor,
        None
    );
}

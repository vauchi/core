// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for api::config
//! Extracted from config.rs

use std::path::PathBuf;
use vauchi_core::api::*;

#[test]
fn test_vauchi_config_default() {
    let config = VauchiConfig::default();

    assert_eq!(config.storage_path, PathBuf::from("./vauchi_data"));
    assert!(config.auto_save);
    assert!(config.relay.server_url.is_empty());
}

#[test]
fn test_vauchi_config_builder() {
    let config = VauchiConfig::with_storage_path("/tmp/test")
        .with_relay_url("wss://relay.example.com")
        .without_auto_save();

    assert_eq!(config.storage_path, PathBuf::from("/tmp/test"));
    assert_eq!(config.relay.server_url, "wss://relay.example.com");
    assert!(!config.auto_save);
}

#[test]
fn test_relay_config_default() {
    let config = RelayConfig::default();

    assert!(config.server_url.is_empty());
    assert_eq!(config.connect_timeout_ms, 10_000);
    assert_eq!(config.io_timeout_ms, 30_000);
    assert_eq!(config.max_reconnect_attempts, 5);
    assert_eq!(config.max_pending_messages, 100);
    assert_eq!(config.ack_timeout_ms, 30_000);
}

#[test]
fn test_relay_config_to_transport_config() {
    let relay = RelayConfig {
        server_url: "wss://test.com".into(),
        connect_timeout_ms: 5_000,
        io_timeout_ms: 15_000,
        max_reconnect_attempts: 3,
        reconnect_base_delay_ms: 500,
        ..Default::default()
    };

    let transport = relay.to_transport_config();

    assert_eq!(transport.server_url, "wss://test.com");
    assert_eq!(transport.connect_timeout_ms, 5_000);
    assert_eq!(transport.io_timeout_ms, 15_000);
    assert_eq!(transport.max_reconnect_attempts, 3);
    assert_eq!(transport.reconnect_base_delay_ms, 500);
}

#[test]
fn test_relay_config_to_relay_client_config() {
    let relay = RelayConfig {
        server_url: "wss://test.com".into(),
        max_pending_messages: 50,
        ack_timeout_ms: 15_000,
        max_retries: 3,
        ..Default::default()
    };

    let client_config = relay.to_relay_client_config();

    assert_eq!(client_config.transport.server_url, "wss://test.com");
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

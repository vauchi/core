// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::transport
//! Extracted from transport.rs

use vauchi_core::network::*;

// @scenario: relay_network :: Relay node configuration
#[test]
fn test_transport_config_defaults() {
    let config = TransportConfig::default();

    assert!(config.server_url.is_empty());
    assert_eq!(config.connect_timeout_ms, 10_000);
    assert_eq!(config.io_timeout_ms, 30_000);
    assert_eq!(config.max_reconnect_attempts, 5);
    assert_eq!(config.reconnect_base_delay_ms, 1_000);
    assert_eq!(config.proxy, ProxyConfig::None);
}

// @scenario: relay_network :: Relay cannot identify users
#[test]
fn test_proxy_config_defaults() {
    let proxy = ProxyConfig::default();
    assert_eq!(proxy, ProxyConfig::None);
}

// @scenario: relay_network :: SOCKS5 proxy support
#[test]
fn test_proxy_config_tor_default() {
    let proxy = ProxyConfig::tor_default();
    assert!(proxy.is_tor());
    if let ProxyConfig::Socks5 { host, port, .. } = proxy {
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9050);
    } else {
        panic!("Expected Socks5 proxy");
    }
}

// @scenario: relay_network :: SOCKS5 proxy support
#[test]
fn test_proxy_config_tor_browser() {
    let proxy = ProxyConfig::tor_browser();
    assert!(proxy.is_tor());
    if let ProxyConfig::Socks5 { port, .. } = proxy {
        assert_eq!(port, 9150);
    } else {
        panic!("Expected Socks5 proxy");
    }
}

// @scenario: relay_network :: SOCKS5 proxy support
#[test]
fn test_proxy_config_socks5_custom() {
    let proxy = ProxyConfig::socks5("192.168.1.1", 1080);
    assert!(!proxy.is_tor()); // Not standard Tor port
    if let ProxyConfig::Socks5 { host, port, .. } = proxy {
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 1080);
    } else {
        panic!("Expected Socks5 proxy");
    }
}

// @scenario: relay_network :: SOCKS5 proxy support
#[test]
fn test_transport_config_with_proxy_timeouts() {
    let config =
        TransportConfig::with_proxy_timeouts("wss://relay.example.com", ProxyConfig::tor_default());

    assert_eq!(config.server_url, "wss://relay.example.com");
    assert!(config.proxy.is_tor());
    // Proxied connections have longer timeouts
    assert_eq!(config.connect_timeout_ms, 60_000);
    assert_eq!(config.io_timeout_ms, 120_000);
}

// @scenario: relay_network :: Relay node configuration
#[test]
fn test_transport_config_with_proxy() {
    let proxy = ProxyConfig::socks5("proxy.example.com", 1080);
    let config = TransportConfig::with_proxy("wss://relay.example.com", proxy);

    assert_eq!(config.server_url, "wss://relay.example.com");
    assert!(!config.proxy.is_tor());
}

// @scenario: relay_network :: Relay node health check
#[test]
fn test_connection_state_equality() {
    assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
    assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
    assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);

    assert_eq!(
        ConnectionState::Reconnecting { attempt: 1 },
        ConnectionState::Reconnecting { attempt: 1 }
    );
    assert_ne!(
        ConnectionState::Reconnecting { attempt: 1 },
        ConnectionState::Reconnecting { attempt: 2 }
    );
}

// @scenario: relay_network :: Relay node health check
#[test]
fn test_connection_state_debug() {
    let state = ConnectionState::Reconnecting { attempt: 3 };
    let debug = format!("{:?}", state);
    assert!(debug.contains("Reconnecting"));
    assert!(debug.contains("3"));
}

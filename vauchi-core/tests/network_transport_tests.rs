//! Tests for network::transport
//! Extracted from transport.rs

use vauchi_core::network::*;

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

#[test]
fn test_proxy_config_defaults() {
    let proxy = ProxyConfig::default();
    assert_eq!(proxy, ProxyConfig::None);
}

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

#[test]
fn test_transport_config_with_tor() {
    let config = TransportConfig::with_tor("wss://relay.example.onion");

    assert_eq!(config.server_url, "wss://relay.example.onion");
    assert!(config.proxy.is_tor());
    // Tor has longer timeouts
    assert_eq!(config.connect_timeout_ms, 60_000);
    assert_eq!(config.io_timeout_ms, 120_000);
}

#[test]
fn test_transport_config_with_proxy() {
    let proxy = ProxyConfig::socks5("proxy.example.com", 1080);
    let config = TransportConfig::with_proxy("wss://relay.example.com", proxy);

    assert_eq!(config.server_url, "wss://relay.example.com");
    assert!(!config.proxy.is_tor());
}

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

#[test]
fn test_connection_state_debug() {
    let state = ConnectionState::Reconnecting { attempt: 3 };
    let debug = format!("{:?}", state);
    assert!(debug.contains("Reconnecting"));
    assert!(debug.contains("3"));
}

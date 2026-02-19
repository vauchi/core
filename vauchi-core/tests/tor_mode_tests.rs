// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Tor mode functionality
//!
//! Traces to: features/tor_mode.feature
//!
//! Covers:
//! - Tor opt-in default state (@opt-in @default)
//! - Circuit rotation (@circuit)
//! - Bootstrap progress (@bootstrap)
//! - Bridge configuration (@bridges)
//! - Onion address fallback (@connection)
//!
//! Note: Requires `testing` feature to access MockTorConnector and TorTransport.

use std::sync::Arc;
use vauchi_core::network::tor::{MockTorConnector, TorConnector, TorTransport};
use vauchi_core::network::{ConnectionState, Transport, TransportConfig};
use vauchi_core::tor_config::{TorConfig, TorRelayAddress, TorStatus};

// =============================================================================
// Tor Opt-In Default Tests
// Traces to: tor_mode.feature @opt-in @default
// Scenario: Tor mode is disabled by default
// =============================================================================

#[test]
fn test_tor_opt_in_default() {
    // Given I have just installed the app (fresh TorConfig)
    let config = TorConfig::default();

    // Then Tor mode should be OFF
    assert!(
        !config.enabled,
        "Tor should be disabled by default (opt-in only)"
    );

    // And connections should use direct networking (no bridges by default)
    assert!(
        config.bridges.is_empty(),
        "No bridges should be configured by default"
    );

    // And no Tor components should be loaded (status is Disabled)
    let connector = MockTorConnector::new();
    assert_eq!(
        connector.status(),
        TorStatus::Disabled,
        "Initial Tor status should be Disabled"
    );
}

#[test]
fn test_tor_config_enabled_requires_explicit_opt_in() {
    // Given Tor mode is disabled (default)
    let default_config = TorConfig::default();
    assert!(!default_config.enabled);

    // When I explicitly enable Tor mode
    let enabled_config = TorConfig::enabled();

    // Then Tor mode should be activated
    assert!(enabled_config.enabled, "Tor should be enabled after opt-in");

    // And default values should be sensible
    assert!(
        enabled_config.prefer_onion,
        "Should prefer onion addresses by default"
    );
    assert_eq!(
        enabled_config.circuit_rotation_secs, 600,
        "Default circuit rotation should be 10 minutes"
    );
}

// =============================================================================
// Circuit Rotation Tests
// Traces to: tor_mode.feature @circuit
// Scenario: Establish new Tor circuit
// Scenario: Automatic circuit rotation
// =============================================================================

#[test]
fn test_tor_circuit_rotation() {
    // Given Tor mode is enabled
    let connector = MockTorConnector::new();

    // And a Tor circuit is established
    connector.bootstrap().expect("Bootstrap should succeed");
    assert_eq!(
        connector.status(),
        TorStatus::Connected,
        "Should be connected after bootstrap"
    );

    // When I request a new circuit
    let result = connector.rotate_circuit();

    // Then a new Tor circuit should be established
    assert!(
        result.is_ok(),
        "Circuit rotation should succeed when connected"
    );

    // And status should remain Connected
    assert_eq!(
        connector.status(),
        TorStatus::Connected,
        "Should remain connected after rotation"
    );
}

#[test]
fn test_tor_circuit_rotation_requires_connection() {
    // Given Tor mode is enabled but not connected
    let connector = MockTorConnector::new();
    assert_eq!(connector.status(), TorStatus::Disabled);

    // When I try to rotate circuit without connecting first
    let result = connector.rotate_circuit();

    // Then the operation should fail
    assert!(
        result.is_err(),
        "Circuit rotation should fail when not connected"
    );
}

#[test]
fn test_tor_circuit_rotation_after_shutdown() {
    // Given Tor was connected but then shutdown
    let connector = MockTorConnector::new();
    connector.bootstrap().unwrap();
    connector.shutdown().unwrap();
    assert_eq!(connector.status(), TorStatus::Disabled);

    // When I try to rotate circuit
    let result = connector.rotate_circuit();

    // Then rotation should fail
    assert!(
        result.is_err(),
        "Circuit rotation should fail after shutdown"
    );
}

// =============================================================================
// Bootstrap Progress Tests
// Traces to: tor_mode.feature @bootstrap
// Scenario: Tor bootstrap progress shown
// Scenario: Tor bootstrap failure handling
// =============================================================================

#[test]
fn test_tor_bootstrap_progress() {
    // Given Tor mode is enabled
    let connector = MockTorConnector::new();

    // And Tor is not yet connected
    assert_eq!(
        connector.status(),
        TorStatus::Disabled,
        "Initial status should be Disabled"
    );

    // When the app starts and bootstraps
    let result = connector.bootstrap();

    // Then I should see Tor bootstrap progress (success = Connected)
    assert!(result.is_ok(), "Bootstrap should succeed");
    assert_eq!(
        connector.status(),
        TorStatus::Connected,
        "Status should be Connected after successful bootstrap"
    );
}

#[test]
fn test_tor_bootstrap_failure_handling() {
    // Given Tor mode is enabled
    // And Tor cannot connect (network blocked)
    let connector = MockTorConnector::failing_bootstrap();

    // When bootstrap fails
    let result = connector.bootstrap();

    // Then I should see "Tor connection failed"
    assert!(result.is_err(), "Bootstrap should fail");

    // And status should remain Disabled (not connected)
    assert_eq!(
        connector.status(),
        TorStatus::Disabled,
        "Status should remain Disabled after failed bootstrap"
    );
}

#[test]
fn test_tor_status_transitions() {
    // Test all valid status transitions

    // Disabled -> Connecting -> Connected (successful bootstrap)
    let connector = MockTorConnector::new();
    assert_eq!(connector.status(), TorStatus::Disabled);

    connector.bootstrap().unwrap();
    assert_eq!(connector.status(), TorStatus::Connected);

    // Connected -> Disabled (shutdown)
    connector.shutdown().unwrap();
    assert_eq!(connector.status(), TorStatus::Disabled);
}

#[test]
fn test_tor_bootstrap_percentage_status() {
    // Test that Bootstrapping status can report percentage
    let status = TorStatus::Bootstrapping { percentage: 75 };

    assert_eq!(status.to_string(), "Bootstrapping (75%)");

    // Verify serialization preserves percentage
    let json = serde_json::to_string(&status).unwrap();
    let restored: TorStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(status, restored);
}

// =============================================================================
// Bridge Configuration Tests
// Traces to: tor_mode.feature @bridges
// Scenario: Configure Tor bridges
// Scenario: Pluggable transports support
// =============================================================================

#[test]
fn test_tor_bridge_configuration() {
    // Given Tor mode is enabled
    // And direct Tor access is blocked
    // When I configure bridge addresses
    let bridge_config = TorConfig::enabled()
        .with_bridges(vec![
            "obfs4 192.168.1.1:443 cert=abc123".to_string(),
            "obfs4 10.0.0.1:9001 cert=def456".to_string(),
        ])
        .unwrap();

    // Then bridges should be configured
    assert!(bridge_config.has_bridges(), "Bridges should be configured");
    assert_eq!(bridge_config.bridges.len(), 2, "Should have 2 bridges");

    // And Tor mode should still be enabled
    assert!(
        bridge_config.enabled,
        "Tor mode should remain enabled with bridges"
    );
}

#[test]
fn test_tor_bridge_configuration_empty_by_default() {
    // Given default config
    let config = TorConfig::default();

    // Then no bridges should be configured
    assert!(!config.has_bridges(), "No bridges by default");
    assert!(config.bridges.is_empty(), "Bridges list should be empty");
}

#[test]
fn test_tor_bridge_configuration_serialization() {
    // Given a config with bridges
    let config = TorConfig::enabled()
        .with_bridges(vec!["obfs4 192.168.1.1:443 cert=test".to_string()])
        .unwrap()
        .with_circuit_rotation_secs(300);

    // When serialized and deserialized
    let json = config.to_json().expect("Serialization should succeed");
    let restored = TorConfig::from_json(&json).expect("Deserialization should succeed");

    // Then all fields should be preserved
    assert_eq!(config.enabled, restored.enabled);
    assert_eq!(config.bridges, restored.bridges);
    assert_eq!(config.prefer_onion, restored.prefer_onion);
    assert_eq!(config.circuit_rotation_secs, restored.circuit_rotation_secs);
}

#[test]
fn test_tor_bridge_validation() {
    // Test bridge address format (obfs4)
    let config = TorConfig::enabled()
        .with_bridges(vec![
            "obfs4 198.51.100.1:443 cert=ABC fingerprint=DEF".to_string(),
        ])
        .unwrap();

    assert!(config.has_bridges());
    assert!(config.bridges[0].starts_with("obfs4"));
}

// =============================================================================
// Onion Address Fallback Tests
// Traces to: tor_mode.feature @connection
// Scenario: Connect to relay .onion address
// Scenario: Fallback to clearnet relay if .onion unavailable
// =============================================================================

#[test]
fn test_onion_address_fallback() {
    // Given Tor mode is enabled
    // And the relay provides both .onion and clearnet addresses
    let relay = TorRelayAddress::with_onion(
        "wss://relay.vauchi.app:443",
        "ws://vauchiexampleonion.onion:80",
    );

    // When prefer_onion is true
    // Then I should connect to the .onion address
    assert_eq!(
        relay.preferred_url(true),
        "ws://vauchiexampleonion.onion:80",
        "Should prefer .onion when available and preferred"
    );

    // When prefer_onion is false
    // Then I should connect to the clearnet address
    assert_eq!(
        relay.preferred_url(false),
        "wss://relay.vauchi.app:443",
        "Should use clearnet when onion not preferred"
    );
}

#[test]
fn test_onion_address_fallback_when_no_onion() {
    // Given the relay only has a clearnet address
    let relay = TorRelayAddress::clearnet("wss://relay.vauchi.app:443");

    // Then even with prefer_onion = true
    // I should fallback to clearnet address
    assert_eq!(
        relay.preferred_url(true),
        "wss://relay.vauchi.app:443",
        "Should fallback to clearnet when no .onion available"
    );

    assert_eq!(
        relay.preferred_url(false),
        "wss://relay.vauchi.app:443",
        "Should use clearnet as normal"
    );
}

#[test]
fn test_onion_address_construction() {
    // Test creating relay addresses with various formats
    let clearnet_only = TorRelayAddress::clearnet("wss://relay.example.com");
    assert!(clearnet_only.onion_url.is_none());
    assert_eq!(clearnet_only.clearnet_url, "wss://relay.example.com");

    let with_onion =
        TorRelayAddress::with_onion("wss://relay.example.com", "ws://example.onion:80");
    assert!(with_onion.onion_url.is_some());
    assert_eq!(
        with_onion.onion_url.as_deref(),
        Some("ws://example.onion:80")
    );
}

#[test]
fn test_relay_address_equality() {
    // Test equality for relay addresses
    let addr1 = TorRelayAddress::with_onion("wss://relay.vauchi.app", "ws://test.onion");
    let addr2 = TorRelayAddress::with_onion("wss://relay.vauchi.app", "ws://test.onion");
    let addr3 = TorRelayAddress::clearnet("wss://relay.vauchi.app");

    assert_eq!(addr1, addr2, "Equal addresses should match");
    assert_ne!(addr1, addr3, "Different addresses should not match");
}

// =============================================================================
// Transport Integration Tests
// Traces to: tor_mode.feature @connection
// Scenario: Relay connections use Tor when enabled
// Scenario: Connection fails gracefully without Tor
// =============================================================================

#[test]
fn test_tor_transport_requires_bootstrap() {
    // Given Tor mode is enabled but not bootstrapped
    let connector = Arc::new(MockTorConnector::new());
    let mut transport = TorTransport::new(connector);

    // When I try to connect without bootstrapping
    let config = TransportConfig {
        server_url: "ws://example.onion:80".to_string(),
        ..Default::default()
    };
    let result = transport.connect(&config);

    // Then connection should fail gracefully
    assert!(result.is_err(), "Connect should fail without bootstrap");
    assert_eq!(
        transport.state(),
        ConnectionState::Disconnected,
        "State should be Disconnected after failed connect"
    );
}

#[test]
fn test_tor_transport_connect_after_bootstrap() {
    // Given Tor mode is enabled and bootstrapped
    let connector = Arc::new(MockTorConnector::new());
    connector.bootstrap().expect("Bootstrap should succeed");

    // When I connect through the transport
    let mut transport = TorTransport::new(connector);
    let config = TransportConfig {
        server_url: "ws://example.onion:80".to_string(),
        ..Default::default()
    };
    let result = transport.connect(&config);

    // Then connection should succeed
    assert!(
        result.is_ok(),
        "Connect should succeed after bootstrap: {:?}",
        result
    );
    assert_eq!(
        transport.state(),
        ConnectionState::Connected,
        "State should be Connected"
    );
}

#[test]
fn test_tor_transport_disconnect() {
    // Given a connected Tor transport
    let connector = Arc::new(MockTorConnector::new());
    connector.bootstrap().unwrap();

    let mut transport = TorTransport::new(connector);
    let config = TransportConfig {
        server_url: "ws://example.onion:80".to_string(),
        ..Default::default()
    };
    transport.connect(&config).unwrap();
    assert_eq!(transport.state(), ConnectionState::Connected);

    // When I disconnect
    let result = transport.disconnect();

    // Then disconnection should succeed
    assert!(result.is_ok(), "Disconnect should succeed");
    assert_eq!(
        transport.state(),
        ConnectionState::Disconnected,
        "State should be Disconnected"
    );
}

#[test]
fn test_tor_connector_lifecycle() {
    // Test the full Tor connector lifecycle
    let connector = MockTorConnector::new();

    // Initial state
    assert_eq!(connector.status(), TorStatus::Disabled);

    // Bootstrap
    connector.bootstrap().unwrap();
    assert_eq!(connector.status(), TorStatus::Connected);

    // Connect (requires bootstrapped state)
    let stream = connector.connect_to("example.onion", 80);
    assert!(stream.is_ok(), "Connect should succeed when bootstrapped");

    // Rotate circuit
    connector.rotate_circuit().unwrap();
    assert_eq!(connector.status(), TorStatus::Connected);

    // Shutdown
    connector.shutdown().unwrap();
    assert_eq!(connector.status(), TorStatus::Disabled);

    // Connect after shutdown should fail
    let stream = connector.connect_to("example.onion", 80);
    assert!(stream.is_err(), "Connect should fail after shutdown");
}

// =============================================================================
// Config Builder Pattern Tests
// =============================================================================

#[test]
fn test_tor_config_builder_chain() {
    // Test fluent builder pattern
    let config = TorConfig::enabled()
        .with_bridges(vec!["198.51.100.1:9001".to_string()])
        .unwrap()
        .with_prefer_onion(false)
        .with_circuit_rotation_secs(120);

    assert!(config.enabled);
    assert_eq!(config.bridges.len(), 1);
    assert!(!config.prefer_onion);
    assert_eq!(config.circuit_rotation_secs, 120);
}

#[test]
fn test_tor_config_custom_circuit_rotation() {
    // Given I want faster circuit rotation for high-security use
    let config = TorConfig::enabled().with_circuit_rotation_secs(60);

    // Then circuit rotation should be 1 minute
    assert_eq!(config.circuit_rotation_secs, 60);
}

#[test]
fn test_tor_config_prefer_onion_toggle() {
    // Given default config prefers onion
    let default = TorConfig::enabled();
    assert!(default.prefer_onion);

    // When I disable onion preference
    let no_onion = TorConfig::enabled().with_prefer_onion(false);

    // Then onion preference should be disabled
    assert!(!no_onion.prefer_onion);
}

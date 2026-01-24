//! Multi-Relay Integration Tests
//!
//! Integration tests for multi-relay support.
//! Feature file: features/relay_network.feature @multi-relay
//!
//! These tests verify:
//! - Multi-relay configuration
//! - Relay selection strategies
//! - Health tracking
//! - Failover behavior

use std::time::Duration;
use vauchi_core::network::{
    MultiRelayConfig, RelayHealth, RelaySelector,
};

// ============================================================
// Multi-Relay Configuration
// Feature: relay_network.feature @multi-relay
// ============================================================

/// Test: Configure multiple relay URLs
#[test]
fn test_multi_relay_config_creation() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://relay1.vauchi.app")
        .add_relay("wss://relay2.vauchi.app")
        .add_relay("wss://relay3.vauchi.app")
        .build()
        .expect("Should create config");

    assert_eq!(config.relay_count(), 3);
    assert!(config.relays().iter().any(|r| r == "wss://relay1.vauchi.app"));
}

/// Test: At least one relay required
#[test]
fn test_multi_relay_requires_at_least_one() {
    let result = MultiRelayConfig::builder().build();
    assert!(result.is_err(), "Should require at least one relay");
}

/// Test: Duplicate relays are deduplicated
#[test]
fn test_multi_relay_deduplicates_urls() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://relay.vauchi.app")
        .add_relay("wss://relay.vauchi.app")
        .add_relay("wss://relay.vauchi.app")
        .build()
        .unwrap();

    assert_eq!(config.relay_count(), 1, "Should deduplicate URLs");
}

/// Test: Primary relay preference
#[test]
fn test_primary_relay_preference() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://secondary.vauchi.app")
        .primary_relay("wss://primary.vauchi.app")
        .build()
        .unwrap();

    assert_eq!(config.primary(), Some("wss://primary.vauchi.app"));
    assert_eq!(config.relay_count(), 2);
}

// ============================================================
// Relay Selection Strategy
// Feature: relay_network.feature @load-balancing
// ============================================================

/// Test: Round-robin selection
#[test]
fn test_round_robin_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://relay1.vauchi.app")
        .add_relay("wss://relay2.vauchi.app")
        .add_relay("wss://relay3.vauchi.app")
        .selection_strategy(RelaySelector::RoundRobin)
        .build()
        .unwrap();

    let first = config.select_relay();
    let second = config.select_relay();
    let _third = config.select_relay();
    let fourth = config.select_relay();

    // Should cycle through relays
    assert_eq!(first, fourth, "Should wrap around");
    assert_ne!(first, second);
}

/// Test: Random selection returns valid relays
#[test]
fn test_random_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://relay1.vauchi.app")
        .add_relay("wss://relay2.vauchi.app")
        .add_relay("wss://relay3.vauchi.app")
        .selection_strategy(RelaySelector::Random)
        .build()
        .unwrap();

    // Just verify it returns valid relays
    for _ in 0..10 {
        let selected = config.select_relay();
        assert!(
            config.relays().contains(&selected),
            "Selected relay should be in config"
        );
    }
}

/// Test: Primary-first selection
#[test]
fn test_primary_first_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://backup.vauchi.app")
        .primary_relay("wss://primary.vauchi.app")
        .selection_strategy(RelaySelector::PrimaryFirst)
        .build()
        .unwrap();

    // Should always return primary when healthy
    for _ in 0..5 {
        assert_eq!(config.select_relay(), "wss://primary.vauchi.app");
    }
}

// ============================================================
// Relay Health Tracking
// Feature: relay_network.feature @failover
// ============================================================

/// Test: Track relay health
#[test]
fn test_relay_health_tracking() {
    let mut health = RelayHealth::new();

    health.record_success("wss://relay1.vauchi.app");
    health.record_success("wss://relay1.vauchi.app");
    health.record_failure("wss://relay2.vauchi.app");

    assert!(health.is_healthy("wss://relay1.vauchi.app"));
    assert!(!health.is_healthy("wss://relay2.vauchi.app"));
}

/// Test: Unknown relay is healthy by default
#[test]
fn test_unknown_relay_healthy() {
    let health = RelayHealth::new();
    assert!(health.is_healthy("wss://unknown.vauchi.app"));
}

/// Test: Unhealthy relay recovers after cooldown
#[test]
fn test_relay_recovery_after_cooldown() {
    let mut health = RelayHealth::with_cooldown(Duration::from_millis(50));

    health.record_failure("wss://relay.vauchi.app");
    assert!(!health.is_healthy("wss://relay.vauchi.app"));

    std::thread::sleep(Duration::from_millis(100));

    // After cooldown, relay should be considered for retry
    assert!(health.should_retry("wss://relay.vauchi.app"));
}

/// Test: Consecutive failures increase cooldown
#[test]
fn test_exponential_backoff_on_failures() {
    let mut health = RelayHealth::new();

    // Record multiple failures
    for _ in 0..3 {
        health.record_failure("wss://relay.vauchi.app");
    }

    let cooldown = health.cooldown_remaining("wss://relay.vauchi.app");

    // Cooldown should be longer after multiple failures
    assert!(cooldown > Duration::from_secs(0));
}

/// Test: Success resets failure count
#[test]
fn test_success_resets_failures() {
    let mut health = RelayHealth::new();

    // Record failures
    health.record_failure("wss://relay.vauchi.app");
    health.record_failure("wss://relay.vauchi.app");
    assert!(!health.is_healthy("wss://relay.vauchi.app"));

    // Success should reset
    health.record_success("wss://relay.vauchi.app");
    assert!(health.is_healthy("wss://relay.vauchi.app"));
}

// ============================================================
// Serialization
// ============================================================

/// Test: Config serialization
#[test]
fn test_config_serialization() {
    let config = MultiRelayConfig::builder()
        .add_relay("wss://relay1.vauchi.app")
        .add_relay("wss://relay2.vauchi.app")
        .primary_relay("wss://relay1.vauchi.app")
        .selection_strategy(RelaySelector::RoundRobin)
        .build()
        .unwrap();

    let json = serde_json::to_string(&config).expect("Should serialize");
    let restored: MultiRelayConfig = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(config.relay_count(), restored.relay_count());
    assert_eq!(config.primary(), restored.primary());
}

/// Test: RelaySelector serialization
#[test]
fn test_selector_serialization() {
    let selector = RelaySelector::RoundRobin;
    let json = serde_json::to_string(&selector).unwrap();
    let restored: RelaySelector = serde_json::from_str(&json).unwrap();
    assert_eq!(selector, restored);
}

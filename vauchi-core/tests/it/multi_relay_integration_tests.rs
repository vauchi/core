// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

use std::time::{Duration, Instant};
use vauchi_core::network::{MultiRelayConfig, RelayHealth, RelaySelector};

// ============================================================
// Multi-Relay Configuration
// Feature: relay_network.feature @multi-relay
// ============================================================

/// Test: Configure multiple relay URLs
// @scenario: relay_network :: Multiple relay nodes for redundancy
// @internal
#[test]
fn test_multi_relay_config_creation() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://relay1.vauchi.app")
        .add_relay("https://relay2.vauchi.app")
        .add_relay("https://relay3.vauchi.app")
        .build()
        .expect("Should create config");

    assert_eq!(config.relay_count(), 3);
    assert!(
        config
            .relays()
            .iter()
            .any(|r| r == "https://relay1.vauchi.app")
    );
}

/// Test: At least one relay required
// @scenario: relay_network :: Multiple relay nodes for redundancy
// @internal
#[test]
fn test_multi_relay_requires_at_least_one() {
    let result = MultiRelayConfig::builder().build();
    assert!(result.is_err(), "Should require at least one relay");
}

/// Test: Duplicate relays are deduplicated
// @scenario: relay_network :: Multiple relay nodes for redundancy
// @internal
#[test]
fn test_multi_relay_deduplicates_urls() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://relay.vauchi.app")
        .add_relay("https://relay.vauchi.app")
        .add_relay("https://relay.vauchi.app")
        .build()
        .unwrap();

    assert_eq!(config.relay_count(), 1, "Should deduplicate URLs");
}

/// Test: Primary relay preference
// @scenario: relay_network :: Prefer specific relay nodes
// @internal
#[test]
fn test_primary_relay_preference() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://secondary.vauchi.app")
        .primary_relay("https://primary.vauchi.app")
        .build()
        .unwrap();

    assert_eq!(config.primary(), Some("https://primary.vauchi.app"));
    assert_eq!(config.relay_count(), 2);
}

// ============================================================
// Relay Selection Strategy
// Feature: relay_network.feature @load-balancing
// ============================================================

/// Test: Round-robin selection
// @scenario: relay_network :: Geographic distribution of relays
// @internal
#[test]
fn test_round_robin_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://relay1.vauchi.app")
        .add_relay("https://relay2.vauchi.app")
        .add_relay("https://relay3.vauchi.app")
        .selection_strategy(RelaySelector::RoundRobin)
        .build()
        .unwrap();

    let first = config.select_relay(&vauchi_core::rng::OsSecureRng::new());
    let second = config.select_relay(&vauchi_core::rng::OsSecureRng::new());
    let _third = config.select_relay(&vauchi_core::rng::OsSecureRng::new());
    let fourth = config.select_relay(&vauchi_core::rng::OsSecureRng::new());

    // Should cycle through relays
    assert_eq!(first, fourth, "Should wrap around");
    assert_ne!(first, second);
}

/// Test: Random selection returns valid relays
// @scenario: relay_network :: Geographic distribution of relays
// @internal
#[test]
fn test_random_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://relay1.vauchi.app")
        .add_relay("https://relay2.vauchi.app")
        .add_relay("https://relay3.vauchi.app")
        .selection_strategy(RelaySelector::Random)
        .build()
        .unwrap();

    // Just verify it returns valid relays
    for _ in 0..10 {
        let selected = config.select_relay(&vauchi_core::rng::OsSecureRng::new());
        assert!(
            config.relays().contains(&selected),
            "Selected relay should be in config"
        );
    }
}

/// Test: Primary-first selection
// @scenario: relay_network :: Prefer specific relay nodes
// @internal
#[test]
fn test_primary_first_selection() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://backup.vauchi.app")
        .primary_relay("https://primary.vauchi.app")
        .selection_strategy(RelaySelector::PrimaryFirst)
        .build()
        .unwrap();

    // Should always return primary when healthy
    for _ in 0..5 {
        assert_eq!(
            config.select_relay(&vauchi_core::rng::OsSecureRng::new()),
            "https://primary.vauchi.app"
        );
    }
}

// ============================================================
// Relay Health Tracking
// Feature: relay_network.feature @failover
// ============================================================

/// Test: Track relay health
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_relay_health_tracking() {
    let mut health = RelayHealth::new();

    health.record_success("https://relay1.vauchi.app");
    health.record_success("https://relay1.vauchi.app");
    health.record_failure("https://relay2.vauchi.app");

    assert!(health.is_healthy("https://relay1.vauchi.app"));
    assert!(!health.is_healthy("https://relay2.vauchi.app"));
}

/// Test: Unknown relay is healthy by default
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_unknown_relay_healthy() {
    let health = RelayHealth::new();
    assert!(health.is_healthy("https://unknown.vauchi.app"));
}

/// Test: Unhealthy relay recovers after cooldown
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_relay_recovery_after_cooldown() {
    let mut health = RelayHealth::with_cooldown(Duration::from_millis(50));

    health.record_failure("https://relay.vauchi.app");
    assert!(!health.is_healthy("https://relay.vauchi.app"));

    // Advance time past the maximum possible cooldown (base=50ms, failures=1,
    // so max cooldown = 50ms, jitter range = [25ms, 50ms]). At 100ms past the
    // failure, any jitter value is exceeded — no sleep needed.
    let future = Instant::now() + Duration::from_millis(100);
    assert!(health.should_retry_at("https://relay.vauchi.app", future));
}

/// Test: Consecutive failures increase cooldown
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_exponential_backoff_on_failures() {
    let mut health = RelayHealth::new();

    // Record multiple failures
    for _ in 0..3 {
        health.record_failure("https://relay.vauchi.app");
    }

    let cooldown = health.cooldown_remaining("https://relay.vauchi.app");

    // Cooldown should be longer after multiple failures
    assert!(cooldown > Duration::from_secs(0));
}

/// Test: Success resets failure count
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_success_resets_failures() {
    let mut health = RelayHealth::new();

    // Record failures
    health.record_failure("https://relay.vauchi.app");
    health.record_failure("https://relay.vauchi.app");
    assert!(!health.is_healthy("https://relay.vauchi.app"));

    // Success should reset
    health.record_success("https://relay.vauchi.app");
    assert!(health.is_healthy("https://relay.vauchi.app"));
}

/// The retry-cooldown gate is driven by the injected `MonotonicClock`
/// (Phase 1 / Task 1.1b), not ambient `Instant::now()`. Exercises the
/// `should_retry()` boundary and the `record_failure` set-site — both
/// route through the seam. Before the migration this fails: advancing
/// the fake clock has no effect because the boundary reads the OS clock.
// @scenario: relay_network :: Relay node health check
// @internal
#[test]
fn test_cooldown_gate_driven_by_injected_monotonic_clock() {
    use std::sync::Arc;
    use vauchi_core::monotonic::FakeMonotonicClock;

    let fake = Arc::new(FakeMonotonicClock::new());
    // base cooldown 50ms; one failure → cooldown in [25ms, 50ms].
    let mut health =
        RelayHealth::with_cooldown(Duration::from_millis(50)).with_monotonic(fake.clone());

    health.record_failure("https://relay.vauchi.app");
    assert!(
        !health.should_retry("https://relay.vauchi.app"),
        "fresh failure must gate retry via the injected clock (elapsed 0 < cooldown)"
    );

    fake.advance(Duration::from_millis(100));
    assert!(
        health.should_retry("https://relay.vauchi.app"),
        "advancing the injected clock past cooldown must release the retry gate"
    );
}

// ============================================================
// Serialization
// ============================================================

/// Test: Config serialization
// @internal
#[test]
fn test_config_serialization() {
    let config = MultiRelayConfig::builder()
        .add_relay("https://relay1.vauchi.app")
        .add_relay("https://relay2.vauchi.app")
        .primary_relay("https://relay1.vauchi.app")
        .selection_strategy(RelaySelector::RoundRobin)
        .build()
        .unwrap();

    let json = serde_json::to_string(&config).expect("Should serialize");
    let restored: MultiRelayConfig = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(config.relay_count(), restored.relay_count());
    assert_eq!(config.primary(), restored.primary());
}

/// Test: RelaySelector serialization
// @internal
#[test]
fn test_selector_serialization() {
    let selector = RelaySelector::RoundRobin;
    let json = serde_json::to_string(&selector).unwrap();
    let restored: RelaySelector = serde_json::from_str(&json).unwrap();
    assert_eq!(selector, restored);
}

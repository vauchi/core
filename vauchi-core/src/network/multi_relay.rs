// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-Relay Support
//!
//! Configuration and management for connecting to multiple relay servers.
//! Provides failover, load balancing, and health tracking.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Multi-relay configuration errors
#[derive(Error, Debug)]
pub enum MultiRelayError {
    #[error("At least one relay URL is required")]
    NoRelays,

    #[error("Invalid relay URL: {0}")]
    InvalidUrl(String),
}

/// Relay selection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RelaySelector {
    /// Cycle through relays in order
    RoundRobin,
    /// Select randomly
    Random,
    /// Always use primary unless unhealthy
    #[default]
    PrimaryFirst,
}

/// Configuration for multiple relay servers
#[derive(Debug, Serialize, Deserialize)]
pub struct MultiRelayConfig {
    /// List of relay URLs
    relays: Vec<String>,
    /// Primary relay (preferred)
    primary: Option<String>,
    /// Selection strategy
    selector: RelaySelector,
    /// Current round-robin index (not serialized)
    #[serde(skip)]
    round_robin_index: AtomicUsize,
}

impl Clone for MultiRelayConfig {
    fn clone(&self) -> Self {
        MultiRelayConfig {
            relays: self.relays.clone(),
            primary: self.primary.clone(),
            selector: self.selector,
            round_robin_index: AtomicUsize::new(self.round_robin_index.load(Ordering::Relaxed)),
        }
    }
}

impl MultiRelayConfig {
    /// Create a new builder
    pub fn builder() -> MultiRelayConfigBuilder {
        MultiRelayConfigBuilder::new()
    }

    /// Get the number of configured relays
    pub fn relay_count(&self) -> usize {
        self.relays.len()
    }

    /// Get all relay URLs
    pub fn relays(&self) -> &[String] {
        &self.relays
    }

    /// Get the primary relay if set
    pub fn primary(&self) -> Option<&str> {
        self.primary.as_deref()
    }

    /// Select a relay based on the configured strategy
    pub fn select_relay(&self) -> String {
        match self.selector {
            RelaySelector::RoundRobin => {
                let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
                self.relays[index % self.relays.len()].clone()
            }
            RelaySelector::Random => {
                let mut rng = rand::thread_rng();
                self.relays.choose(&mut rng).unwrap().clone()
            }
            RelaySelector::PrimaryFirst => {
                if let Some(primary) = &self.primary {
                    primary.clone()
                } else {
                    self.relays[0].clone()
                }
            }
        }
    }

    /// Select a relay, excluding unhealthy ones
    pub fn select_healthy_relay(&self, health: &RelayHealth) -> Option<String> {
        match self.selector {
            RelaySelector::PrimaryFirst => {
                // Try primary first
                if let Some(primary) = &self.primary {
                    if health.is_healthy(primary) {
                        return Some(primary.clone());
                    }
                }
                // Fall back to first healthy
                self.relays.iter().find(|r| health.is_healthy(r)).cloned()
            }
            RelaySelector::RoundRobin => {
                // Find next healthy relay in round-robin order
                let start = self.round_robin_index.load(Ordering::Relaxed);
                for i in 0..self.relays.len() {
                    let index = (start + i) % self.relays.len();
                    if health.is_healthy(&self.relays[index]) {
                        self.round_robin_index.store(index + 1, Ordering::Relaxed);
                        return Some(self.relays[index].clone());
                    }
                }
                None
            }
            RelaySelector::Random => {
                let healthy: Vec<_> = self
                    .relays
                    .iter()
                    .filter(|r| health.is_healthy(r))
                    .collect();
                if healthy.is_empty() {
                    None
                } else {
                    let mut rng = rand::thread_rng();
                    Some(healthy.choose(&mut rng).unwrap().to_string())
                }
            }
        }
    }
}

/// Builder for MultiRelayConfig
#[derive(Debug, Default)]
pub struct MultiRelayConfigBuilder {
    relays: HashSet<String>,
    primary: Option<String>,
    selector: RelaySelector,
}

impl MultiRelayConfigBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        MultiRelayConfigBuilder::default()
    }

    /// Add a relay URL
    pub fn add_relay(mut self, url: &str) -> Self {
        self.relays.insert(url.to_string());
        self
    }

    /// Set the primary relay (also adds it to the list)
    pub fn primary_relay(mut self, url: &str) -> Self {
        self.relays.insert(url.to_string());
        self.primary = Some(url.to_string());
        self
    }

    /// Set the selection strategy
    pub fn selection_strategy(mut self, selector: RelaySelector) -> Self {
        self.selector = selector;
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<MultiRelayConfig, MultiRelayError> {
        if self.relays.is_empty() {
            return Err(MultiRelayError::NoRelays);
        }

        let relays: Vec<String> = self.relays.into_iter().collect();

        Ok(MultiRelayConfig {
            relays,
            primary: self.primary,
            selector: self.selector,
            round_robin_index: AtomicUsize::new(0),
        })
    }
}

/// Health state for a single relay
#[derive(Debug, Clone, Default)]
struct RelayHealthState {
    /// Number of consecutive failures
    failure_count: u32,
    /// Last failure time
    last_failure: Option<Instant>,
    /// Last success time
    last_success: Option<Instant>,
}

/// Tracks health status of relay servers
#[derive(Debug)]
pub struct RelayHealth {
    /// Health state per relay
    states: HashMap<String, RelayHealthState>,
    /// Base cooldown duration
    base_cooldown: Duration,
    /// Maximum cooldown duration
    max_cooldown: Duration,
}

impl Default for RelayHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayHealth {
    /// Create a new health tracker with default settings
    pub fn new() -> Self {
        RelayHealth {
            states: HashMap::new(),
            base_cooldown: Duration::from_secs(5),
            max_cooldown: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Create a health tracker with custom cooldown
    pub fn with_cooldown(base_cooldown: Duration) -> Self {
        RelayHealth {
            states: HashMap::new(),
            base_cooldown,
            max_cooldown: Duration::from_secs(300),
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self, relay: &str) {
        let state = self.states.entry(relay.to_string()).or_default();
        state.failure_count = 0;
        state.last_failure = None;
        state.last_success = Some(Instant::now());
    }

    /// Record a failed operation
    pub fn record_failure(&mut self, relay: &str) {
        let state = self.states.entry(relay.to_string()).or_default();
        state.failure_count += 1;
        state.last_failure = Some(Instant::now());
    }

    /// Check if a relay is considered healthy
    pub fn is_healthy(&self, relay: &str) -> bool {
        match self.states.get(relay) {
            None => true, // Unknown relays are assumed healthy
            Some(state) => {
                if state.failure_count == 0 {
                    return true;
                }
                // Check if cooldown has elapsed
                self.should_retry(relay)
            }
        }
    }

    /// Check if we should retry a failed relay
    pub fn should_retry(&self, relay: &str) -> bool {
        match self.states.get(relay) {
            None => true,
            Some(state) => {
                if state.failure_count == 0 {
                    return true;
                }
                match state.last_failure {
                    None => true,
                    Some(last_failure) => {
                        let cooldown = self.calculate_cooldown(state.failure_count);
                        Instant::now().duration_since(last_failure) >= cooldown
                    }
                }
            }
        }
    }

    /// Get remaining cooldown time for a relay
    pub fn cooldown_remaining(&self, relay: &str) -> Duration {
        match self.states.get(relay) {
            None => Duration::ZERO,
            Some(state) => {
                if state.failure_count == 0 {
                    return Duration::ZERO;
                }
                match state.last_failure {
                    None => Duration::ZERO,
                    Some(last_failure) => {
                        let cooldown = self.calculate_cooldown(state.failure_count);
                        let elapsed = Instant::now().duration_since(last_failure);
                        cooldown.saturating_sub(elapsed)
                    }
                }
            }
        }
    }

    /// Calculate cooldown with exponential backoff and jitter (#122).
    ///
    /// Returns a duration in `[max/2, max]` where `max = base * 2^(n-1)` capped
    /// at `self.max_cooldown`. The half-range uniform jitter prevents thundering
    /// herd when multiple clients reconnect after a relay outage.
    fn calculate_cooldown(&self, failure_count: u32) -> Duration {
        let multiplier = 2u64.saturating_pow(failure_count.saturating_sub(1));
        let max_cooldown = self
            .base_cooldown
            .saturating_mul(multiplier as u32)
            .min(self.max_cooldown);
        let max_ms = max_cooldown.as_millis() as u64;
        if max_ms == 0 {
            return Duration::ZERO;
        }
        let half = max_ms / 2;
        let jittered = half + (rand::Rng::gen_range(&mut rand::thread_rng(), 0..=half));
        Duration::from_millis(jittered)
    }
}

/// Lightweight relay selection and health manager.
///
/// Wraps a `MultiRelayConfig` and `RelayHealth` to provide relay selection
/// with automatic failover. Unlike `MultiRelayClient`, this does not manage
/// connections — the caller picks a URL and creates/reuses a transport.
pub struct MultiRelayManager {
    config: MultiRelayConfig,
    health: RelayHealth,
}

impl MultiRelayManager {
    /// Create a new manager from a configuration.
    pub fn new(config: MultiRelayConfig) -> Self {
        MultiRelayManager {
            config,
            health: RelayHealth::new(),
        }
    }

    /// Select the best healthy relay based on the configured strategy.
    /// Returns `None` if all relays are unhealthy.
    pub fn select_relay(&self) -> Option<String> {
        self.config.select_healthy_relay(&self.health)
    }

    /// Mark a relay as healthy after a successful operation.
    pub fn mark_healthy(&mut self, relay: &str) {
        self.health.record_success(relay);
    }

    /// Mark a relay as unhealthy after a failed operation.
    pub fn mark_unhealthy(&mut self, relay: &str) {
        self.health.record_failure(relay);
    }

    /// Check if a specific relay is considered healthy.
    pub fn is_relay_healthy(&self, relay: &str) -> bool {
        self.health.is_healthy(relay)
    }

    /// Get all configured relay URLs.
    pub fn all_relays(&self) -> &[String] {
        self.config.relays()
    }

    /// Get the number of configured relays.
    pub fn relay_count(&self) -> usize {
        self.config.relay_count()
    }

    /// Get a reference to the underlying config.
    pub fn config(&self) -> &MultiRelayConfig {
        &self.config
    }

    /// Get a reference to the health tracker.
    pub fn health(&self) -> &RelayHealth {
        &self.health
    }
}

/// Client for connecting to multiple relay servers with failover support.
///
/// **Status (#115):** Relay selection and health tracking are fully implemented.
/// The `send_raw()` and `receive_pending()` methods are stubs — they do not
/// open real WebSocket connections. For production use, `RelayClient` provides
/// the actual single-relay transport. This struct is a future extension point
/// for multi-relay failover when the protocol supports it.
pub struct MultiRelayClient {
    /// Configuration
    config: MultiRelayConfig,
    /// Health tracker
    health: RelayHealth,
    /// Our identity ID (used when sending messages)
    #[allow(dead_code)]
    identity_id: String,
    /// Currently active relay URL
    active_relay: Option<String>,
    /// Connection state
    connected: bool,
    /// Mock mode for testing
    mock_mode: bool,
    /// Simulated failures (for testing)
    simulated_failures: HashSet<String>,
    /// Queued incoming messages (for testing)
    incoming_queue: Vec<Vec<u8>>,
}

impl MultiRelayClient {
    /// Create a new multi-relay client
    pub fn new(config: MultiRelayConfig, identity_id: String) -> Self {
        MultiRelayClient {
            config,
            health: RelayHealth::new(),
            identity_id,
            active_relay: None,
            connected: false,
            mock_mode: false,
            simulated_failures: HashSet::new(),
            incoming_queue: Vec::new(),
        }
    }

    /// Create a client with mock transports for testing
    pub fn with_mock_transports(config: MultiRelayConfig, identity_id: String) -> Self {
        MultiRelayClient {
            config,
            health: RelayHealth::new(),
            identity_id,
            active_relay: None,
            connected: false,
            mock_mode: true,
            simulated_failures: HashSet::new(),
            incoming_queue: Vec::new(),
        }
    }

    /// Get the number of configured relays
    pub fn relay_count(&self) -> usize {
        self.config.relay_count()
    }

    /// Check if connected to any relay
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the currently active relay URL
    pub fn active_relay(&self) -> Option<String> {
        self.active_relay.clone()
    }

    /// Connect to the best available relay
    pub fn connect(&mut self) -> Result<(), MultiRelayError> {
        // Mark simulated failures
        for relay in self.config.relays() {
            if self.simulated_failures.contains(relay) {
                self.health.record_failure(relay);
            }
        }

        // Try to find a healthy relay using the configured strategy
        if let Some(relay) = self.config.select_healthy_relay(&self.health) {
            // In mock mode, just mark as connected
            self.active_relay = Some(relay.clone());
            self.connected = true;
            self.health.record_success(&relay);
            return Ok(());
        }

        Err(MultiRelayError::NoRelays)
    }

    /// Disconnect from the current relay
    pub fn disconnect(&mut self) -> Result<(), MultiRelayError> {
        self.active_relay = None;
        self.connected = false;
        Ok(())
    }

    /// Send a raw message to a recipient
    pub fn send_raw(&mut self, recipient_id: &str, data: &[u8]) -> Result<(), MultiRelayError> {
        if !self.connected {
            return Err(MultiRelayError::NoRelays);
        }

        // In mock mode, just pretend we sent it
        if self.mock_mode {
            return Ok(());
        }

        // Real send would happen here
        let _ = (recipient_id, data);
        Ok(())
    }

    /// Receive pending messages
    pub fn receive_pending(&mut self) -> Result<Vec<Vec<u8>>, MultiRelayError> {
        if !self.connected {
            return Err(MultiRelayError::NoRelays);
        }

        // In mock mode, return queued messages
        if self.mock_mode {
            let messages = std::mem::take(&mut self.incoming_queue);
            return Ok(messages);
        }

        // Real receive would happen here
        Ok(Vec::new())
    }

    /// Simulate a relay failure (for testing)
    pub fn simulate_relay_failure(&mut self, relay: &str) {
        self.simulated_failures.insert(relay.to_string());
        self.health.record_failure(relay);
    }

    /// Queue an incoming message (for testing)
    pub fn queue_incoming_message(&mut self, data: &[u8]) {
        self.incoming_queue.push(data.to_vec());
    }

    /// Get the health tracker (for advanced use)
    pub fn health(&self) -> &RelayHealth {
        &self.health
    }

    /// Get mutable health tracker
    pub fn health_mut(&mut self) -> &mut RelayHealth {
        &mut self.health
    }
}

// INLINE_TEST_REQUIRED: tests need access to private RelayHealthState fields and calculate_cooldown
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = MultiRelayConfig::builder()
            .add_relay("wss://relay1.test")
            .add_relay("wss://relay2.test")
            .build()
            .unwrap();

        assert_eq!(config.relay_count(), 2);
    }

    #[test]
    fn test_empty_config_fails() {
        let result = MultiRelayConfig::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_health_tracking() {
        let mut health = RelayHealth::new();

        health.record_success("relay1");
        assert!(health.is_healthy("relay1"));

        health.record_failure("relay2");
        assert!(!health.is_healthy("relay2"));
    }

    #[test]
    fn test_multi_relay_manager_select_primary_first() {
        let config = MultiRelayConfig::builder()
            .primary_relay("wss://primary.test")
            .add_relay("wss://backup.test")
            .build()
            .unwrap();

        let manager = MultiRelayManager::new(config);
        let selected = manager.select_relay();

        assert_eq!(
            selected,
            Some("wss://primary.test".to_string()),
            "PrimaryFirst strategy should select the primary relay"
        );
    }

    #[test]
    fn test_multi_relay_manager_fallback_on_unhealthy() {
        let config = MultiRelayConfig::builder()
            .primary_relay("wss://primary.test")
            .add_relay("wss://backup.test")
            .build()
            .unwrap();

        let mut manager = MultiRelayManager::new(config);
        manager.mark_unhealthy("wss://primary.test");

        let selected = manager.select_relay();
        assert_eq!(
            selected,
            Some("wss://backup.test".to_string()),
            "Should fall back to backup when primary is unhealthy"
        );
    }

    #[test]
    fn test_multi_relay_manager_marks_health() {
        let config = MultiRelayConfig::builder()
            .add_relay("wss://relay1.test")
            .build()
            .unwrap();

        let mut manager = MultiRelayManager::new(config);

        assert!(
            manager.is_relay_healthy("wss://relay1.test"),
            "Unknown relay should be considered healthy"
        );

        manager.mark_unhealthy("wss://relay1.test");
        assert!(
            !manager.is_relay_healthy("wss://relay1.test"),
            "Should be unhealthy after marking"
        );

        manager.mark_healthy("wss://relay1.test");
        assert!(
            manager.is_relay_healthy("wss://relay1.test"),
            "Should be healthy again after marking healthy"
        );
    }

    #[test]
    fn test_multi_relay_manager_all_unhealthy_returns_none() {
        let config = MultiRelayConfig::builder()
            .primary_relay("wss://relay1.test")
            .add_relay("wss://relay2.test")
            .build()
            .unwrap();

        let mut manager = MultiRelayManager::new(config);
        manager.mark_unhealthy("wss://relay1.test");
        manager.mark_unhealthy("wss://relay2.test");

        let selected = manager.select_relay();
        assert_eq!(
            selected, None,
            "Should return None when all relays are unhealthy"
        );
    }

    #[test]
    fn test_exponential_backoff_with_jitter() {
        let health = RelayHealth::new();

        // Sample many cooldowns to verify statistical properties
        let samples = 200;
        let mut c1_total = 0u128;
        let mut c3_total = 0u128;
        for _ in 0..samples {
            c1_total += health.calculate_cooldown(1).as_millis();
            c3_total += health.calculate_cooldown(3).as_millis();
        }
        // Average cooldown at failure_count=3 should exceed failure_count=1
        let avg1 = c1_total / samples as u128;
        let avg3 = c3_total / samples as u128;
        assert!(
            avg3 > avg1,
            "avg cooldown(3)={avg3}ms should exceed cooldown(1)={avg1}ms"
        );

        // Verify jitter: cooldown(1) should be in [base/2, base] = [2500, 5000]ms
        let c = health.calculate_cooldown(1);
        assert!(c.as_millis() >= 2500, "jitter floor: {c:?}");
        assert!(c.as_millis() <= 5000, "jitter ceiling: {c:?}");
    }
}

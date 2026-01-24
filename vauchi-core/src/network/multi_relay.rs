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

    /// Calculate cooldown based on failure count (exponential backoff)
    fn calculate_cooldown(&self, failure_count: u32) -> Duration {
        let multiplier = 2u64.saturating_pow(failure_count.saturating_sub(1));
        let cooldown = self.base_cooldown.saturating_mul(multiplier as u32);
        cooldown.min(self.max_cooldown)
    }
}

/// Client for connecting to multiple relay servers with failover support
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
    fn test_exponential_backoff() {
        let health = RelayHealth::new();

        let c1 = health.calculate_cooldown(1);
        let c2 = health.calculate_cooldown(2);
        let c3 = health.calculate_cooldown(3);

        assert!(c2 > c1);
        assert!(c3 > c2);
    }
}

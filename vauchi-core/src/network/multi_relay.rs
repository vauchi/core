// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-Relay Support
//!
//! Configuration and management for connecting to multiple relay servers.
//! Provides failover, load balancing, and health tracking.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::contact::Contact;

/// Multi-relay configuration errors
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum MultiRelayError {
    #[error("At least one relay URL is required")]
    NoRelays,

    #[error("Invalid relay URL: {0}")]
    InvalidUrl(String),
}

/// Relay selection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
                // Non-crypto RNG: relay load balancing, not security-sensitive
                let mut rng = rand::thread_rng();
                self.relays
                    .choose(&mut rng)
                    .expect("relays list is non-empty (validated at construction)")
                    .clone()
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
                if let Some(primary) = &self.primary
                    && health.is_healthy(primary)
                {
                    return Some(primary.clone());
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
                    // Non-crypto RNG: relay load balancing, not security-sensitive
                    let mut rng = rand::thread_rng();
                    Some(
                        healthy
                            .choose(&mut rng)
                            .expect("healthy is non-empty (checked above)")
                            .to_string(),
                    )
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
        self.should_retry_at(relay, Instant::now())
    }

    /// Check if we should retry a failed relay at a given point in time.
    pub fn should_retry_at(&self, relay: &str, now: Instant) -> bool {
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
                        now.duration_since(last_failure) >= cooldown
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
        // Non-crypto RNG: cooldown jitter for thundering herd prevention
        let jittered = half + (rand::Rng::gen_range(&mut rand::thread_rng(), 0..=half));
        Duration::from_millis(jittered)
    }
}

/// Lightweight relay selection and health manager.
///
/// Wraps a `MultiRelayConfig` and `RelayHealth` to provide relay selection
/// with automatic failover. Supports per-contact relay routing: contacts
/// may specify their own relay URL, and the manager tracks health for those
/// relays too.
pub struct MultiRelayManager {
    config: MultiRelayConfig,
    health: RelayHealth,
    /// Relay URLs learned from contact exchanges (deduplicated).
    contact_relays: HashSet<String>,
}

impl MultiRelayManager {
    /// Create a new manager from a configuration.
    pub fn new(config: MultiRelayConfig) -> Self {
        MultiRelayManager {
            config,
            health: RelayHealth::new(),
            contact_relays: HashSet::new(),
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

    // ========================================
    // Per-Contact Relay Routing
    // ========================================

    /// Register a relay URL learned from a contact exchange.
    ///
    /// The relay is tracked for health monitoring. Duplicate URLs are ignored.
    pub fn add_contact_relay(&mut self, url: &str) {
        self.contact_relays.insert(url.to_string());
    }

    /// Returns the relay URL to use for a specific contact.
    ///
    /// If the contact has a relay URL set and that relay is healthy,
    /// returns the contact's relay. Otherwise falls back to the home relay
    /// (primary from config).
    pub fn relay_for_contact(&self, contact: &Contact) -> String {
        if let Some(contact_relay) = contact.relay_url()
            && !contact_relay.is_empty()
            && self.health.is_healthy(contact_relay)
        {
            return contact_relay.to_string();
        }
        // Fall back to home relay
        self.config
            .select_healthy_relay(&self.health)
            .unwrap_or_else(|| {
                // Last resort: return primary even if unhealthy
                self.config
                    .primary()
                    .unwrap_or_else(|| {
                        self.config
                            .relays()
                            .first()
                            .map(|s| s.as_str())
                            .unwrap_or("")
                    })
                    .to_string()
            })
    }

    /// Returns all known contact relay URLs.
    pub fn contact_relay_urls(&self) -> Vec<&str> {
        self.contact_relays.iter().map(|s| s.as_str()).collect()
    }

    /// Returns all unique relay URLs (home relays + contact relays).
    pub fn all_relay_urls(&self) -> Vec<String> {
        let mut urls: HashSet<String> = self.config.relays().iter().cloned().collect();
        for url in &self.contact_relays {
            urls.insert(url.clone());
        }
        urls.into_iter().collect()
    }

    /// Groups contacts by their target relay URL.
    ///
    /// Returns a map from relay URL to list of contact display names.
    /// Contacts without a relay URL are grouped under the home relay.
    pub fn group_contacts_by_relay<'a>(
        &self,
        contacts: &'a [Contact],
    ) -> BTreeMap<String, Vec<&'a str>> {
        let home_relay = self
            .config
            .primary()
            .unwrap_or_else(|| {
                self.config
                    .relays()
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("")
            })
            .to_string();

        let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for contact in contacts {
            let relay = contact
                .relay_url()
                .filter(|url| !url.is_empty())
                .unwrap_or(&home_relay);
            groups
                .entry(relay.to_string())
                .or_default()
                .push(contact.display_name());
        }
        groups
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
        result.expect_err("expected error");
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

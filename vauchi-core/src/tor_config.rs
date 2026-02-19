// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor Configuration Types
//!
//! Data types for Tor connectivity configuration. These types are
//! available unconditionally (no feature flags) so they can be used
//! by storage operations even when the network module is not compiled.
//! The actual Tor transport implementation is in `network::tor`.

use serde::{Deserialize, Serialize};

/// Current status of the Tor connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TorStatus {
    /// Tor is not enabled.
    Disabled,
    /// Tor client is connecting to the network.
    Connecting,
    /// Tor client is bootstrapping (downloading directory info).
    Bootstrapping {
        /// Bootstrap progress percentage (0-100).
        percentage: u8,
    },
    /// Tor client is connected and ready.
    Connected,
    /// Tor client is disconnected.
    Disconnected {
        /// Reason for disconnection.
        reason: String,
    },
}

impl std::fmt::Display for TorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TorStatus::Disabled => write!(f, "Disabled"),
            TorStatus::Connecting => write!(f, "Connecting"),
            TorStatus::Bootstrapping { percentage } => {
                write!(f, "Bootstrapping ({}%)", percentage)
            }
            TorStatus::Connected => write!(f, "Connected"),
            TorStatus::Disconnected { reason } => write!(f, "Disconnected: {}", reason),
        }
    }
}

/// Configuration for Tor connectivity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorConfig {
    /// Whether Tor mode is enabled.
    pub enabled: bool,
    /// Bridge addresses for censored networks (obfs4 format).
    pub bridges: Vec<String>,
    /// Whether to prefer .onion addresses when available.
    pub prefer_onion: bool,
    /// How often to rotate Tor circuits (in seconds). Default: 600 (10 minutes).
    pub circuit_rotation_secs: u64,
}

impl Default for TorConfig {
    fn default() -> Self {
        TorConfig {
            enabled: false,
            bridges: Vec::new(),
            prefer_onion: true,
            circuit_rotation_secs: 600,
        }
    }
}

impl TorConfig {
    /// Creates a new TorConfig with Tor enabled.
    pub fn enabled() -> Self {
        TorConfig {
            enabled: true,
            ..Default::default()
        }
    }

    /// Builder: set bridge addresses (#107).
    ///
    /// Each bridge must be non-empty and contain a transport keyword followed
    /// by an IP:port (e.g., `obfs4 198.51.100.1:443 cert=...`).
    /// Returns an error if any bridge string is malformed.
    pub fn with_bridges(mut self, bridges: Vec<String>) -> Result<Self, String> {
        for bridge in &bridges {
            Self::validate_bridge(bridge)?;
        }
        self.bridges = bridges;
        Ok(self)
    }

    /// Validates a single bridge address string (#107).
    ///
    /// Accepted formats:
    /// - `obfs4 IP:PORT cert=... iat-mode=N`
    /// - `IP:PORT` (plain bridge)
    fn validate_bridge(bridge: &str) -> Result<(), String> {
        let bridge = bridge.trim();
        if bridge.is_empty() {
            return Err("bridge address cannot be empty".to_string());
        }

        // Split into parts. First token is either a transport or an IP:PORT.
        let parts: Vec<&str> = bridge.split_whitespace().collect();
        if parts.is_empty() {
            return Err("bridge address cannot be empty".to_string());
        }

        // Check if first part is a known transport
        let addr_part = match parts[0] {
            "obfs4" | "obfs3" | "meek_lite" | "snowflake" | "webtunnel" => {
                if parts.len() < 2 {
                    return Err(format!(
                        "bridge transport '{}' requires at least an IP:PORT",
                        parts[0]
                    ));
                }
                parts[1]
            }
            _ => parts[0], // Assume it's a plain IP:PORT bridge
        };

        // Validate that addr_part looks like IP:PORT
        if !addr_part.contains(':') {
            return Err(format!(
                "bridge address '{}' missing port (expected IP:PORT)",
                addr_part
            ));
        }

        // Validate port is numeric
        if let Some(port_str) = addr_part.rsplit(':').next() {
            if port_str.parse::<u16>().is_err() {
                return Err(format!(
                    "bridge port '{}' is not a valid port number",
                    port_str
                ));
            }
        }

        Ok(())
    }

    /// Builder: set .onion preference.
    pub fn with_prefer_onion(mut self, prefer: bool) -> Self {
        self.prefer_onion = prefer;
        self
    }

    /// Builder: set circuit rotation interval.
    pub fn with_circuit_rotation_secs(mut self, secs: u64) -> Self {
        self.circuit_rotation_secs = secs;
        self
    }

    /// Returns true if bridges are configured for censored networks.
    pub fn has_bridges(&self) -> bool {
        !self.bridges.is_empty()
    }

    /// Serializes to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserializes from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// A relay address that may have both clearnet and .onion URLs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorRelayAddress {
    /// The clearnet URL (e.g. wss://relay.vauchi.app).
    pub clearnet_url: String,
    /// The optional .onion URL.
    pub onion_url: Option<String>,
}

impl TorRelayAddress {
    /// Creates a new relay address with only a clearnet URL.
    pub fn clearnet(url: impl Into<String>) -> Self {
        TorRelayAddress {
            clearnet_url: url.into(),
            onion_url: None,
        }
    }

    /// Creates a new relay address with both clearnet and .onion URLs.
    pub fn with_onion(clearnet_url: impl Into<String>, onion_url: impl Into<String>) -> Self {
        TorRelayAddress {
            clearnet_url: clearnet_url.into(),
            onion_url: Some(onion_url.into()),
        }
    }

    /// Returns the preferred URL based on the onion preference.
    ///
    /// If `prefer_onion` is true and an .onion URL is available, returns it.
    /// Otherwise returns the clearnet URL.
    pub fn preferred_url(&self, prefer_onion: bool) -> &str {
        if prefer_onion {
            self.onion_url.as_deref().unwrap_or(&self.clearnet_url)
        } else {
            &self.clearnet_url
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_config_defaults() {
        let config = TorConfig::default();
        assert!(!config.enabled);
        assert!(config.bridges.is_empty());
        assert!(config.prefer_onion);
        assert_eq!(config.circuit_rotation_secs, 600);
    }

    #[test]
    fn test_tor_config_enabled() {
        let config = TorConfig::enabled();
        assert!(config.enabled);
        assert!(config.prefer_onion);
    }

    #[test]
    fn test_tor_config_builder() {
        let config = TorConfig::enabled()
            .with_bridges(vec![
                "obfs4 198.51.100.1:443 cert=abc iat-mode=0".to_string(),
                "obfs4 198.51.100.2:9001 cert=def iat-mode=1".to_string(),
            ])
            .unwrap()
            .with_prefer_onion(false)
            .with_circuit_rotation_secs(300);

        assert!(config.enabled);
        assert_eq!(config.bridges.len(), 2);
        assert!(!config.prefer_onion);
        assert_eq!(config.circuit_rotation_secs, 300);
    }

    #[test]
    fn test_tor_config_has_bridges() {
        let config = TorConfig::default();
        assert!(!config.has_bridges());

        let config = TorConfig::enabled()
            .with_bridges(vec!["198.51.100.1:9001".to_string()])
            .unwrap();
        assert!(config.has_bridges());
    }

    #[test]
    fn test_bridge_validation_rejects_empty() {
        let result = TorConfig::enabled().with_bridges(vec!["".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_bridge_validation_rejects_no_port() {
        let result = TorConfig::enabled().with_bridges(vec!["obfs4 198.51.100.1".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_bridge_validation_rejects_invalid_port() {
        let result = TorConfig::enabled().with_bridges(vec!["obfs4 198.51.100.1:abc".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_bridge_validation_accepts_obfs4() {
        let result = TorConfig::enabled().with_bridges(vec![
            "obfs4 198.51.100.1:443 cert=abcdef iat-mode=0".to_string(),
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bridge_validation_accepts_plain_bridge() {
        let result = TorConfig::enabled().with_bridges(vec!["198.51.100.1:9001".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tor_config_serialization() {
        let config = TorConfig::enabled()
            .with_bridges(vec!["obfs4 192.168.1.1:443 cert=test".to_string()])
            .unwrap()
            .with_circuit_rotation_secs(300);

        let json = config.to_json().unwrap();
        let restored = TorConfig::from_json(&json).unwrap();

        assert_eq!(config.enabled, restored.enabled);
        assert_eq!(config.bridges, restored.bridges);
        assert_eq!(config.prefer_onion, restored.prefer_onion);
        assert_eq!(config.circuit_rotation_secs, restored.circuit_rotation_secs);
    }

    #[test]
    fn test_tor_status_display() {
        assert_eq!(TorStatus::Disabled.to_string(), "Disabled");
        assert_eq!(TorStatus::Connecting.to_string(), "Connecting");
        assert_eq!(
            TorStatus::Bootstrapping { percentage: 50 }.to_string(),
            "Bootstrapping (50%)"
        );
        assert_eq!(TorStatus::Connected.to_string(), "Connected");
        assert_eq!(
            TorStatus::Disconnected {
                reason: "timeout".to_string()
            }
            .to_string(),
            "Disconnected: timeout"
        );
    }

    #[test]
    fn test_tor_status_serialization() {
        let status = TorStatus::Bootstrapping { percentage: 75 };
        let json = serde_json::to_string(&status).unwrap();
        let restored: TorStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, restored);
    }

    #[test]
    fn test_relay_address_clearnet_only() {
        let addr = TorRelayAddress::clearnet("wss://relay.vauchi.app");
        assert_eq!(addr.clearnet_url, "wss://relay.vauchi.app");
        assert!(addr.onion_url.is_none());
    }

    #[test]
    fn test_relay_address_with_onion() {
        let addr =
            TorRelayAddress::with_onion("wss://relay.vauchi.app", "ws://vauchiexample.onion");
        assert_eq!(addr.clearnet_url, "wss://relay.vauchi.app");
        assert_eq!(addr.onion_url.as_deref(), Some("ws://vauchiexample.onion"));
    }

    #[test]
    fn test_relay_address_preferred_url_with_onion() {
        let addr =
            TorRelayAddress::with_onion("wss://relay.vauchi.app", "ws://vauchiexample.onion");
        assert_eq!(addr.preferred_url(true), "ws://vauchiexample.onion");
        assert_eq!(addr.preferred_url(false), "wss://relay.vauchi.app");
    }

    #[test]
    fn test_relay_address_preferred_url_no_onion() {
        let addr = TorRelayAddress::clearnet("wss://relay.vauchi.app");
        assert_eq!(addr.preferred_url(true), "wss://relay.vauchi.app");
        assert_eq!(addr.preferred_url(false), "wss://relay.vauchi.app");
    }
}

//! API Configuration
//!
//! Configuration types for the WebBook API layer.

use std::path::PathBuf;

use crate::crypto::SymmetricKey;
use crate::network::{ProxyConfig, RelayClientConfig, TransportConfig};

/// Configuration for WebBook instance.
#[derive(Debug, Clone)]
pub struct WebBookConfig {
    /// Storage directory for identity, contacts, and sync state.
    pub storage_path: PathBuf,

    /// Relay server configuration.
    pub relay: RelayConfig,

    /// Sync configuration.
    pub sync: SyncConfig,

    /// Auto-save configuration.
    pub auto_save: bool,

    /// Storage encryption key.
    /// If None, a random key will be generated (not persistent across sessions).
    pub storage_key: Option<SymmetricKey>,
}

impl Default for WebBookConfig {
    fn default() -> Self {
        WebBookConfig {
            storage_path: PathBuf::from("./webbook_data"),
            relay: RelayConfig::default(),
            sync: SyncConfig::default(),
            auto_save: true,
            storage_key: None,
        }
    }
}

impl WebBookConfig {
    /// Creates a new configuration with the given storage path.
    pub fn with_storage_path(storage_path: impl Into<PathBuf>) -> Self {
        WebBookConfig {
            storage_path: storage_path.into(),
            ..Default::default()
        }
    }

    /// Sets the relay server URL.
    pub fn with_relay_url(mut self, url: impl Into<String>) -> Self {
        self.relay.server_url = url.into();
        self
    }

    /// Disables auto-save.
    pub fn without_auto_save(mut self) -> Self {
        self.auto_save = false;
        self
    }

    /// Sets the storage encryption key.
    /// Use this to persist data across sessions.
    pub fn with_storage_key(mut self, key: SymmetricKey) -> Self {
        self.storage_key = Some(key);
        self
    }
}

/// Relay server configuration.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Relay server URL.
    pub server_url: String,

    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,

    /// Read/write timeout in milliseconds.
    pub io_timeout_ms: u64,

    /// Maximum reconnection attempts.
    pub max_reconnect_attempts: u32,

    /// Base delay for exponential backoff (milliseconds).
    pub reconnect_base_delay_ms: u64,

    /// Maximum concurrent pending messages.
    pub max_pending_messages: usize,

    /// Acknowledgment timeout in milliseconds.
    pub ack_timeout_ms: u64,

    /// Maximum message retries before giving up.
    pub max_retries: u32,

    /// Proxy configuration (for Tor support).
    pub proxy: ProxyConfig,
}

impl Default for RelayConfig {
    fn default() -> Self {
        RelayConfig {
            server_url: String::new(),
            connect_timeout_ms: 10_000,
            io_timeout_ms: 30_000,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1_000,
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 5,
            proxy: ProxyConfig::None,
        }
    }
}

impl RelayConfig {
    /// Creates a relay config for Tor connections.
    pub fn with_tor(server_url: &str) -> Self {
        RelayConfig {
            server_url: server_url.to_string(),
            // Tor connections are slower
            connect_timeout_ms: 60_000,
            io_timeout_ms: 120_000,
            max_reconnect_attempts: 3,
            reconnect_base_delay_ms: 5_000,
            proxy: ProxyConfig::tor_default(),
            ..Default::default()
        }
    }

    /// Converts to TransportConfig for the network layer.
    pub fn to_transport_config(&self) -> TransportConfig {
        TransportConfig {
            server_url: self.server_url.clone(),
            connect_timeout_ms: self.connect_timeout_ms,
            io_timeout_ms: self.io_timeout_ms,
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_base_delay_ms: self.reconnect_base_delay_ms,
            proxy: self.proxy.clone(),
        }
    }

    /// Converts to RelayClientConfig for the network layer.
    pub fn to_relay_client_config(&self) -> RelayClientConfig {
        RelayClientConfig {
            transport: self.to_transport_config(),
            max_pending_messages: self.max_pending_messages,
            ack_timeout_ms: self.ack_timeout_ms,
            max_retries: self.max_retries,
        }
    }
}

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Automatically sync on contact card changes.
    pub auto_sync: bool,

    /// Sync interval in milliseconds (0 = manual only).
    pub sync_interval_ms: u64,

    /// Maximum pending updates before forcing sync.
    pub max_pending_updates: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            auto_sync: true,
            sync_interval_ms: 60_000, // 1 minute
            max_pending_updates: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webbook_config_default() {
        let config = WebBookConfig::default();

        assert_eq!(config.storage_path, PathBuf::from("./webbook_data"));
        assert!(config.auto_save);
        assert!(config.relay.server_url.is_empty());
    }

    #[test]
    fn test_webbook_config_builder() {
        let config = WebBookConfig::with_storage_path("/tmp/test")
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
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport Trait
//!
//! Platform-agnostic abstraction for network communication.

use super::error::NetworkError;
use super::message::MessageEnvelope;
use super::pinning::PinnedCertificate;

/// Result type for transport operations.
pub type TransportResult<T> = Result<T, NetworkError>;

/// Connection state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    /// Not connected to any server.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Connection failed, will retry.
    Reconnecting { attempt: u32 },
}

/// Proxy configuration for transport connections.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ProxyConfig {
    /// No proxy (direct connection).
    #[default]
    None,
    /// SOCKS5 proxy (for Tor, VPN, or any SOCKS5-compatible proxy).
    Socks5 {
        /// Proxy host address.
        host: String,
        /// Proxy port.
        port: u16,
        /// Optional username for authentication.
        username: Option<String>,
        /// Optional password for authentication.
        password: Option<String>,
    },
    /// HTTP CONNECT proxy.
    HttpConnect {
        /// Proxy host address.
        host: String,
        /// Proxy port.
        port: u16,
    },
}

impl ProxyConfig {
    /// Creates a SOCKS5 proxy config for local Tor (127.0.0.1:9050).
    pub fn tor_default() -> Self {
        ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 9050,
            username: None,
            password: None,
        }
    }

    /// Creates a SOCKS5 proxy config for Tor Browser (127.0.0.1:9150).
    pub fn tor_browser() -> Self {
        ProxyConfig::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 9150,
            username: None,
            password: None,
        }
    }

    /// Creates a custom SOCKS5 proxy config.
    pub fn socks5(host: &str, port: u16) -> Self {
        ProxyConfig::Socks5 {
            host: host.to_string(),
            port,
            username: None,
            password: None,
        }
    }

    /// Returns true if this proxy uses a standard Tor SOCKS5 port.
    pub fn is_tor(&self) -> bool {
        matches!(
            self,
            ProxyConfig::Socks5 {
                port: 9050 | 9150,
                ..
            }
        )
    }
}

/// Configuration for transport connections.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Server URL/address.
    pub server_url: String,
    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Read/write timeout in milliseconds.
    pub io_timeout_ms: u64,
    /// Maximum reconnection attempts.
    pub max_reconnect_attempts: u32,
    /// Base delay for exponential backoff (milliseconds).
    pub reconnect_base_delay_ms: u64,
    /// Proxy configuration (optional SOCKS5 or HTTP CONNECT proxy).
    pub proxy: ProxyConfig,
    /// Relay's Noise NK public key for inner transport encryption.
    /// When set, the transport performs a Noise NK handshake after WebSocket
    /// connect and wraps all subsequent messages with Noise encryption.
    pub relay_noise_pubkey: Option<[u8; 32]>,
    /// Pinned relay certificates for TLS certificate pinning.
    /// When non-empty, the TLS handshake verifies the server's leaf certificate
    /// matches at least one pinned SHA-256 fingerprint. Empty means no pinning.
    pub pinned_certs: Vec<PinnedCertificate>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig {
            server_url: String::new(),
            connect_timeout_ms: 10_000,
            io_timeout_ms: 30_000,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1_000,
            proxy: ProxyConfig::None,
            relay_noise_pubkey: None,
            pinned_certs: Vec::new(),
        }
    }
}

impl TransportConfig {
    /// Creates a config with longer timeouts for proxied connections.
    pub fn with_proxy_timeouts(server_url: &str, proxy: ProxyConfig) -> Self {
        TransportConfig {
            server_url: server_url.to_string(),
            // Proxied connections are slower, use longer timeouts
            connect_timeout_ms: 60_000,
            io_timeout_ms: 120_000,
            max_reconnect_attempts: 3,
            reconnect_base_delay_ms: 5_000,
            proxy,
            relay_noise_pubkey: None,
            pinned_certs: Vec::new(),
        }
    }

    /// Creates a config with a custom proxy.
    pub fn with_proxy(server_url: &str, proxy: ProxyConfig) -> Self {
        TransportConfig {
            server_url: server_url.to_string(),
            proxy,
            ..Default::default()
        }
    }
}

/// Transport trait for network communication.
///
/// This trait abstracts the underlying transport mechanism (WebSocket, TCP, etc.)
/// allowing for platform-specific implementations and easy testing with mocks.
///
/// # Synchronous Interface
///
/// This trait uses synchronous methods for simplicity in the core library.
/// Platform implementations may internally use async runtimes but expose
/// a blocking interface here.
///
/// # Example
///
/// ```ignore
/// use vauchi_core::network::{Transport, MockTransport, TransportConfig};
///
/// let mut transport = MockTransport::new();
/// transport.connect(&TransportConfig::default())?;
/// transport.send(&message)?;
/// let response = transport.receive()?;
/// transport.disconnect()?;
/// ```
pub trait Transport: Send {
    /// Connects to the relay server.
    ///
    /// Returns `Ok(())` on successful connection.
    fn connect(&mut self, config: &TransportConfig) -> TransportResult<()>;

    /// Disconnects from the relay server.
    ///
    /// Safe to call even if not connected.
    fn disconnect(&mut self) -> TransportResult<()>;

    /// Returns the current connection state.
    fn state(&self) -> ConnectionState;

    /// Sends a message envelope to the relay.
    ///
    /// This is a blocking call that waits for the send to complete.
    /// Returns an error if not connected.
    fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()>;

    /// Receives the next message from the relay.
    ///
    /// This is a blocking call that waits for a message or timeout.
    /// Returns `Ok(None)` if no message is available (non-blocking check
    /// or timeout without error).
    fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>>;

    /// Checks if there are pending messages to receive (non-blocking).
    fn has_pending(&self) -> bool;

    /// Sends pre-serialized bytes (with length prefix) to the relay.
    ///
    /// Used for handshakes where the serialization format differs from the
    /// standard `MessageEnvelope` format (e.g., hex-encoded fields for relay
    /// authentication). Default implementation delegates to `send()` after
    /// deserializing, but transport implementations should override this
    /// to send the raw bytes directly.
    fn send_raw(&mut self, data: &[u8]) -> TransportResult<()> {
        // Default: deserialize and send via the normal path.
        // Real transport implementations should override this to avoid
        // the roundtrip deserialization.
        let json = &data[4..]; // Skip length prefix
        let envelope: MessageEnvelope = serde_json::from_slice(json)
            .map_err(|e| super::error::NetworkError::Serialization(e.to_string()))?;
        self.send(&envelope)
    }
}

// Compile-time assertion: Transport must remain object-safe so that
// Vauchi can store `Box<dyn Transport>` without generic parameters (ADR-030).
const _: fn() = || {
    fn _assert_object_safe(_: &dyn Transport) {}
};

//! Transport Trait
//!
//! Platform-agnostic abstraction for network communication.

use super::error::NetworkError;
use super::message::MessageEnvelope;

/// Result type for transport operations.
pub type TransportResult<T> = Result<T, NetworkError>;

/// Connection state.
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub enum ProxyConfig {
    /// No proxy (direct connection).
    #[default]
    None,
    /// SOCKS5 proxy (used for Tor).
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

    /// Returns true if this is a Tor-compatible proxy.
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
    /// Proxy configuration (for Tor support).
    pub proxy: ProxyConfig,
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
        }
    }
}

impl TransportConfig {
    /// Creates a config for connecting via Tor.
    pub fn with_tor(server_url: &str) -> Self {
        TransportConfig {
            server_url: server_url.to_string(),
            // Tor connections are slower, use longer timeouts
            connect_timeout_ms: 60_000,
            io_timeout_ms: 120_000,
            max_reconnect_attempts: 3,
            reconnect_base_delay_ms: 5_000,
            proxy: ProxyConfig::tor_default(),
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
}

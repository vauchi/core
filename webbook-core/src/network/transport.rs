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
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig {
            server_url: String::new(),
            connect_timeout_ms: 10_000,
            io_timeout_ms: 30_000,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1_000,
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
/// use webbook_core::network::{Transport, MockTransport, TransportConfig};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_config_defaults() {
        let config = TransportConfig::default();

        assert!(config.server_url.is_empty());
        assert_eq!(config.connect_timeout_ms, 10_000);
        assert_eq!(config.io_timeout_ms, 30_000);
        assert_eq!(config.max_reconnect_attempts, 5);
        assert_eq!(config.reconnect_base_delay_ms, 1_000);
    }

    #[test]
    fn test_connection_state_equality() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);

        assert_eq!(
            ConnectionState::Reconnecting { attempt: 1 },
            ConnectionState::Reconnecting { attempt: 1 }
        );
        assert_ne!(
            ConnectionState::Reconnecting { attempt: 1 },
            ConnectionState::Reconnecting { attempt: 2 }
        );
    }

    #[test]
    fn test_connection_state_debug() {
        let state = ConnectionState::Reconnecting { attempt: 3 };
        let debug = format!("{:?}", state);
        assert!(debug.contains("Reconnecting"));
        assert!(debug.contains("3"));
    }
}

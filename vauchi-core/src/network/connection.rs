// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Connection Manager
//!
//! Manages connection lifecycle with automatic reconnection and handshake.

use ring::rand::{SecureRandom, SystemRandom};

use super::error::NetworkError;
use super::message::{Handshake, MessageEnvelope, MessagePayload};
use super::protocol::create_envelope;
use super::transport::{ConnectionState, Transport, TransportConfig, TransportResult};
use crate::identity::Identity;

/// Connection manager with automatic reconnection and handshake.
///
/// Wraps a transport implementation and adds:
/// - Automatic reconnection with exponential backoff
/// - Authentication handshake on connect
/// - Connection state management
///
/// # Example
///
/// ```ignore
/// use vauchi_core::network::{ConnectionManager, MockTransport, TransportConfig};
///
/// let transport = MockTransport::new();
/// let config = TransportConfig {
///     server_url: "wss://relay.example.com".into(),
///     ..Default::default()
/// };
///
/// let mut conn = ConnectionManager::new(transport, config);
/// conn.set_identity(identity);
/// conn.connect()?;
/// ```
pub struct ConnectionManager<T: Transport> {
    transport: T,
    config: TransportConfig,
    identity: Option<Identity>,
    reconnect_attempt: u32,
}

impl<T: Transport> ConnectionManager<T> {
    /// Creates a new connection manager.
    pub fn new(transport: T, config: TransportConfig) -> Self {
        ConnectionManager {
            transport,
            config,
            identity: None,
            reconnect_attempt: 0,
        }
    }

    /// Sets the identity for authenticated connections.
    ///
    /// If set, a handshake will be performed on connect.
    pub fn set_identity(&mut self, identity: Identity) {
        self.identity = Some(identity);
    }

    /// Establishes connection and performs handshake if identity is set.
    pub fn connect(&mut self) -> TransportResult<()> {
        self.transport.connect(&self.config)?;
        self.reconnect_attempt = 0;

        // Perform handshake if identity is set
        if self.identity.is_some() {
            self.send_handshake()?;
        }

        Ok(())
    }

    /// Disconnects from the server.
    pub fn disconnect(&mut self) -> TransportResult<()> {
        self.transport.disconnect()
    }

    /// Returns the current connection state.
    pub fn state(&self) -> ConnectionState {
        self.transport.state()
    }

    /// Returns true if connected and ready.
    pub fn is_connected(&self) -> bool {
        self.transport.state() == ConnectionState::Connected
    }

    /// Sends a message, handling reconnection if needed.
    pub fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()> {
        self.ensure_connected()?;
        self.transport.send(message)
    }

    /// Receives a message, handling reconnection if needed.
    pub fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        self.ensure_connected()?;
        self.transport.receive()
    }

    /// Checks if there are pending messages.
    pub fn has_pending(&self) -> bool {
        self.transport.has_pending()
    }

    /// Attempts to reconnect with exponential backoff.
    ///
    /// Returns error if max retries exceeded.
    pub fn reconnect(&mut self) -> TransportResult<()> {
        if self.reconnect_attempt >= self.config.max_reconnect_attempts {
            return Err(NetworkError::MaxRetriesExceeded);
        }

        // Calculate backoff delay (not actually sleeping here - that's for the caller)
        let _delay_ms = self.config.reconnect_base_delay_ms * (1 << self.reconnect_attempt.min(6));

        self.reconnect_attempt += 1;

        // Disconnect and reconnect
        let _ = self.transport.disconnect(); // Ignore disconnect errors
        self.connect()
    }

    /// Returns the current reconnect attempt count.
    pub fn reconnect_attempt(&self) -> u32 {
        self.reconnect_attempt
    }

    /// Resets the reconnect attempt counter.
    pub fn reset_reconnect_count(&mut self) {
        self.reconnect_attempt = 0;
    }

    /// Returns a reference to the underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Returns a mutable reference to the underlying transport.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Ensures the connection is established, reconnecting if needed.
    fn ensure_connected(&mut self) -> TransportResult<()> {
        match self.transport.state() {
            ConnectionState::Connected => Ok(()),
            ConnectionState::Disconnected | ConnectionState::Reconnecting { .. } => {
                self.reconnect()
            }
            ConnectionState::Connecting => {
                // Connection in progress - can't proceed yet
                Err(NetworkError::NotConnected)
            }
        }
    }

    /// Sends the authentication handshake message.
    fn send_handshake(&mut self) -> TransportResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| NetworkError::AuthenticationFailed("No identity set".into()))?;

        let rng = SystemRandom::new();
        let mut nonce = [0u8; 32];
        rng.fill(&mut nonce)
            .map_err(|_| NetworkError::AuthenticationFailed("RNG failed".into()))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        // Sign (nonce || timestamp)
        let mut sign_data = Vec::with_capacity(40);
        sign_data.extend_from_slice(&nonce);
        sign_data.extend_from_slice(&timestamp.to_be_bytes());
        let signature = identity.sign(&sign_data);

        let handshake = Handshake {
            identity_public_key: *identity.signing_public_key(),
            nonce,
            signature: *signature.as_bytes(),
        };

        let envelope = create_envelope(MessagePayload::Handshake(handshake));
        self.transport.send(&envelope)
    }
}

// INLINE_TEST_REQUIRED: Tests private reconnect_attempt field and internal state transitions
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::mock::MockTransport;

    fn create_test_config() -> TransportConfig {
        TransportConfig {
            server_url: "test://localhost".into(),
            max_reconnect_attempts: 3,
            ..Default::default()
        }
    }

    #[test]
    fn test_connection_manager_connect_disconnect() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        assert_eq!(conn.state(), ConnectionState::Disconnected);

        conn.connect().unwrap();
        assert_eq!(conn.state(), ConnectionState::Connected);
        assert!(conn.is_connected());

        conn.disconnect().unwrap();
        assert_eq!(conn.state(), ConnectionState::Disconnected);
        assert!(!conn.is_connected());
    }

    #[test]
    fn test_connection_manager_reconnect_on_failure() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        conn.connect().unwrap();

        // Simulate disconnect
        conn.transport_mut()
            .set_state(ConnectionState::Disconnected);

        // Send should trigger reconnect
        let msg = create_envelope(MessagePayload::Presence(
            crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Online,
                message: None,
            },
        ));

        conn.send(&msg).unwrap();
        assert!(conn.is_connected());
    }

    #[test]
    fn test_connection_manager_max_retries() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        // Manually set the reconnect counter to max
        // This simulates having exhausted all retry attempts
        conn.reconnect_attempt = conn.config.max_reconnect_attempts;

        // Next attempt should fail with MaxRetriesExceeded
        let result = conn.reconnect();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NetworkError::MaxRetriesExceeded
        ));
    }

    #[test]
    fn test_connection_manager_handshake() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        let identity = Identity::create("Test User");
        conn.set_identity(identity);

        conn.connect().unwrap();

        // Check that a handshake message was sent
        let sent = conn.transport().sent_messages();
        assert_eq!(sent.len(), 1);

        if let MessagePayload::Handshake(h) = &sent[0].payload {
            assert_ne!(h.nonce, [0u8; 32]); // Should be random
            assert_ne!(h.signature, [0u8; 64]); // Should be signed
        } else {
            panic!("Expected handshake message");
        }
    }

    #[test]
    fn test_connection_manager_send_receive() {
        let mut transport = MockTransport::new();

        // Queue a message to receive
        let incoming = create_envelope(MessagePayload::Presence(
            crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Away,
                message: Some("BRB".into()),
            },
        ));
        transport.queue_receive(incoming.clone());

        let mut conn = ConnectionManager::new(transport, create_test_config());
        conn.connect().unwrap();

        // Receive
        let received = conn.receive().unwrap().unwrap();
        assert_eq!(received.message_id, incoming.message_id);
    }

    #[test]
    fn test_connection_manager_reset_reconnect_count() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        // Simulate some failed reconnects
        conn.reconnect_attempt = 2;

        conn.reset_reconnect_count();
        assert_eq!(conn.reconnect_attempt(), 0);
    }

    #[test]
    fn test_connection_manager_has_pending() {
        let mut transport = MockTransport::new();
        transport.queue_receive(create_envelope(MessagePayload::Presence(
            crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Online,
                message: None,
            },
        )));

        let mut conn = ConnectionManager::new(transport, create_test_config());
        conn.connect().unwrap();

        assert!(conn.has_pending());
        conn.receive().unwrap();
        assert!(!conn.has_pending());
    }
}

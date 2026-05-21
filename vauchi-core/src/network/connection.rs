// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Connection Manager
//!
//! Manages connection lifecycle with automatic reconnection and handshake.

use super::error::NetworkError;
use super::message::MessageEnvelope;
use super::transport::{ConnectionState, Transport, TransportConfig, TransportResult};
use crate::identity::Identity;

/// Converts a byte slice to a lowercase hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

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
///     server_url: "https://relay.example.com".into(),
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
    suppress_presence: bool,
    /// Clock used to stamp the handshake timestamp. Defaults to
    /// `crate::clock::SystemClock::shared()`; tests can override via
    /// `with_clock(...)` after construction.
    /// Phase 1 / Task 1.1 / Step 3b structural pass.
    clock: std::sync::Arc<dyn crate::clock::Clock>,
    /// Sleeper used for reconnect-backoff suspension. Defaults to
    /// `crate::sleeper::SystemSleeper::shared()`; tests can override
    /// via `with_sleeper(...)` to skip the real wall-clock wait.
    /// Phase 1 / Task 1.3 of the pure-functional-core program.
    sleeper: std::sync::Arc<dyn crate::sleeper::Sleeper>,
}

impl<T: Transport> ConnectionManager<T> {
    /// Creates a new connection manager.
    pub fn new(transport: T, config: TransportConfig) -> Self {
        ConnectionManager {
            transport,
            config,
            identity: None,
            reconnect_attempt: 0,
            suppress_presence: false,
            clock: crate::clock::SystemClock::shared(),
            sleeper: crate::sleeper::SystemSleeper::shared(),
        }
    }

    /// Replaces the manager's clock — for deterministic tests.
    ///
    /// Defaults to `SystemClock::shared()` from `new()`; tests can
    /// override post-construction. Phase 1 / Task 1.1 / Step 3b
    /// structural pass.
    pub fn with_clock(mut self, clock: std::sync::Arc<dyn crate::clock::Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces the manager's sleeper — for deterministic, fast tests.
    ///
    /// Defaults to `SystemSleeper::shared()` from `new()`; tests
    /// can override post-construction (`FakeSleeper` from
    /// `crate::sleeper` records requested durations and returns
    /// immediately, so reconnect-backoff tests run at memory
    /// speed). Phase 1 / Task 1.3 of the pure-functional-core program.
    pub fn with_sleeper(mut self, sleeper: std::sync::Arc<dyn crate::sleeper::Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// Sets whether to suppress presence notifications at the relay.
    pub fn set_suppress_presence(&mut self, suppress: bool) {
        self.suppress_presence = suppress;
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
    /// Sleeps for an exponentially increasing delay before each attempt.
    /// Returns error if max retries exceeded.
    pub fn reconnect(&mut self) -> TransportResult<()> {
        if self.reconnect_attempt >= self.config.max_reconnect_attempts {
            return Err(NetworkError::MaxRetriesExceeded);
        }

        // Exponential backoff: base_delay * 2^attempt, capped at 2^6 = 64x
        let delay_ms =
            self.config.reconnect_base_delay_ms * (1u64 << self.reconnect_attempt.min(6));
        self.sleeper
            .sleep(std::time::Duration::from_millis(delay_ms));

        self.reconnect_attempt += 1;

        // Disconnect and reconnect — disconnect errors are ignored
        // because we're about to drop the transport state anyway
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.transport.disconnect();
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
    ///
    /// Builds a relay-compatible handshake with hex-encoded fields. The relay
    /// uses hex encoding for all byte fields (`client_id`, `identity_public_key`,
    /// `nonce`, `signature`) while the core `Handshake` struct uses base64.
    /// To avoid a serialization mismatch, this builds a `serde_json::Value`
    /// directly with hex-encoded fields.
    fn send_handshake(&mut self) -> TransportResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| NetworkError::AuthenticationFailed("No identity set".into()))?;

        let nonce: [u8; 32] = crate::crypto::random_bytes();

        let timestamp = self.clock.unix_seconds();

        // Sign (nonce || timestamp)
        let mut sign_data = Vec::with_capacity(40);
        sign_data.extend_from_slice(&nonce);
        sign_data.extend_from_slice(&timestamp.to_be_bytes());
        let signature = identity.sign(&sign_data);

        // Build relay-compatible handshake as JSON with hex-encoded fields.
        // The relay expects client_id = hex(public_key), and all byte fields as hex strings.
        let public_key = identity.signing_public_key();
        let client_id = bytes_to_hex(public_key);

        let relay_handshake = serde_json::json!({
            "version": super::message::PROTOCOL_VERSION,
            "message_id": uuid::Uuid::new_v4().to_string(),
            "timestamp": timestamp,
            "payload": {
                "type": "Handshake",
                "client_id": client_id,
                "identity_public_key": client_id,
                "nonce": bytes_to_hex(&nonce),
                "signature": bytes_to_hex(signature.as_bytes()),
                "timestamp": timestamp,
                "suppress_presence": self.suppress_presence,
            }
        });

        let json = serde_json::to_vec(&relay_handshake)
            .map_err(|e| NetworkError::Serialization(e.to_string()))?;

        let len = json.len() as u32;
        let mut frame = Vec::with_capacity(4 + json.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&json);

        self.transport.send_raw(&frame)
    }
}

// INLINE_TEST_REQUIRED: Tests private reconnect_attempt field and internal state transitions
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::MessagePayload;
    use crate::network::mock::MockTransport;
    use crate::network::protocol::create_envelope;

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
        let msg = create_envelope(
            MessagePayload::Presence(crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Online,
                message: None,
            }),
            0,
        );

        conn.send(&msg).unwrap();
        assert!(conn.is_connected());
    }

    // @internal
    #[test]
    fn test_connection_manager_reconnect_uses_injected_sleeper() {
        use crate::sleeper::{FakeSleeper, Sleeper};
        use std::sync::Arc;
        use std::time::Duration;

        let transport = MockTransport::new();
        let fake: Arc<FakeSleeper> = Arc::new(FakeSleeper::new());
        let mut conn = ConnectionManager::new(transport, create_test_config())
            .with_sleeper(fake.clone() as Arc<dyn Sleeper>);

        conn.connect().unwrap();
        // reconnect_attempt starts at 0 after connect() → backoff =
        // base_delay_ms * 2^0 = base_delay_ms. Default test config does
        // not override base_delay_ms, so the value is 1000ms (TransportConfig::default()).
        conn.reconnect().unwrap();

        // Real wall-clock impact: 0ms — FakeSleeper returns immediately.
        // The injected seam captured the call. (reconnect_attempt is
        // not asserted: a successful reconnect calls connect() which
        // resets the counter back to 0 — the sleep happened before
        // that reset, which is the property under test.)
        assert_eq!(fake.calls(), vec![Duration::from_millis(1000)]);
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
        assert!(result.is_err(), "expected error");
        assert!(matches!(
            result.unwrap_err(),
            NetworkError::MaxRetriesExceeded
        ));
    }

    #[test]
    fn test_connection_manager_handshake() {
        let transport = MockTransport::new();
        let mut conn = ConnectionManager::new(transport, create_test_config());

        let identity = Identity::create("Test User", 0);
        let public_key_hex: String = identity
            .signing_public_key()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        conn.set_identity(identity);

        conn.connect().unwrap();

        // Check that a raw handshake was sent (relay-compatible format)
        let sent_raw = conn.transport().sent_raw();
        assert_eq!(sent_raw.len(), 1);

        // Decode: skip 4-byte length prefix, parse JSON
        let json: serde_json::Value = serde_json::from_slice(&sent_raw[0][4..]).unwrap();

        assert_eq!(json["payload"]["type"], "Handshake");
        assert_eq!(json["payload"]["client_id"], public_key_hex);
        assert_eq!(json["payload"]["identity_public_key"], public_key_hex);
        // nonce should be a 64-char hex string (32 bytes)
        let nonce_hex = json["payload"]["nonce"].as_str().unwrap();
        assert_eq!(nonce_hex.len(), 64);
        // signature should be a 128-char hex string (64 bytes)
        let sig_hex = json["payload"]["signature"].as_str().unwrap();
        assert_eq!(sig_hex.len(), 128);
        // timestamp should be present
        assert!(json["payload"]["timestamp"].is_u64());
    }

    #[test]
    fn test_connection_manager_send_receive() {
        let mut transport = MockTransport::new();

        // Queue a message to receive
        let incoming = create_envelope(
            MessagePayload::Presence(crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Away,
                message: Some("BRB".into()),
            }),
            0,
        );
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
        transport.queue_receive(create_envelope(
            MessagePayload::Presence(crate::network::message::PresenceUpdate {
                status: crate::network::message::PresenceStatus::Online,
                message: None,
            }),
            0,
        ));

        let mut conn = ConnectionManager::new(transport, create_test_config());
        conn.connect().unwrap();

        assert!(conn.has_pending());
        conn.receive().unwrap();
        assert!(!conn.has_pending());
    }
}

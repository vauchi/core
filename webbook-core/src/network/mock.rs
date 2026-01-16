//! Mock Transport
//!
//! Mock implementation of the Transport trait for testing.

use std::collections::VecDeque;

use super::error::NetworkError;
use super::message::{
    AckStatus, Acknowledgment, MessageEnvelope, MessagePayload, PROTOCOL_VERSION,
};
use super::transport::{ConnectionState, Transport, TransportConfig, TransportResult};

/// Mock transport for testing.
///
/// Allows injection of responses and tracking of sent messages.
///
/// # Example
///
/// ```ignore
/// use webbook_core::network::{MockTransport, TransportConfig, Transport};
///
/// let mut transport = MockTransport::new();
/// transport.connect(&TransportConfig::default()).unwrap();
///
/// // Queue a message to be returned by receive()
/// transport.queue_receive(some_message);
///
/// // Send a message
/// transport.send(&outgoing_message).unwrap();
///
/// // Check what was sent
/// assert_eq!(transport.sent_messages().len(), 1);
/// ```
#[derive(Debug)]
pub struct MockTransport {
    state: ConnectionState,
    /// Messages that have been sent.
    sent_messages: Vec<MessageEnvelope>,
    /// Messages to return on receive().
    receive_queue: VecDeque<MessageEnvelope>,
    /// Error to inject on next operation.
    inject_error: Option<NetworkError>,
    /// Whether to auto-acknowledge messages.
    auto_ack: bool,
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTransport {
    /// Creates a new mock transport.
    pub fn new() -> Self {
        MockTransport {
            state: ConnectionState::Disconnected,
            sent_messages: Vec::new(),
            receive_queue: VecDeque::new(),
            inject_error: None,
            auto_ack: false,
        }
    }

    /// Queues a message to be returned by the next receive() call.
    pub fn queue_receive(&mut self, message: MessageEnvelope) {
        self.receive_queue.push_back(message);
    }

    /// Returns all messages that have been sent.
    pub fn sent_messages(&self) -> &[MessageEnvelope] {
        &self.sent_messages
    }

    /// Clears the sent messages buffer.
    pub fn clear_sent(&mut self) {
        self.sent_messages.clear();
    }

    /// Injects an error to be returned on the next operation.
    pub fn inject_error(&mut self, error: NetworkError) {
        self.inject_error = Some(error);
    }

    /// Enables auto-acknowledgment mode (generates acks for sent messages).
    pub fn set_auto_ack(&mut self, enabled: bool) {
        self.auto_ack = enabled;
    }

    /// Manually sets the connection state (for testing state transitions).
    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    /// Returns the number of messages in the receive queue.
    pub fn receive_queue_len(&self) -> usize {
        self.receive_queue.len()
    }

    fn check_error(&mut self) -> TransportResult<()> {
        if let Some(err) = self.inject_error.take() {
            return Err(err);
        }
        Ok(())
    }
}

impl Transport for MockTransport {
    fn connect(&mut self, _config: &TransportConfig) -> TransportResult<()> {
        self.check_error()?;
        self.state = ConnectionState::Connected;
        Ok(())
    }

    fn disconnect(&mut self) -> TransportResult<()> {
        self.check_error()?;
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()> {
        self.check_error()?;

        if self.state != ConnectionState::Connected {
            return Err(NetworkError::NotConnected);
        }

        self.sent_messages.push(message.clone());

        // Auto-generate acknowledgment if enabled
        if self.auto_ack {
            let ack = MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: uuid::Uuid::new_v4().to_string(),
                timestamp: message.timestamp,
                payload: MessagePayload::Acknowledgment(Acknowledgment {
                    message_id: message.message_id.clone(),
                    status: AckStatus::Delivered,
                    error: None,
                }),
            };
            self.receive_queue.push_back(ack);
        }

        Ok(())
    }

    fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        self.check_error()?;

        if self.state != ConnectionState::Connected {
            return Err(NetworkError::NotConnected);
        }

        Ok(self.receive_queue.pop_front())
    }

    fn has_pending(&self) -> bool {
        !self.receive_queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::{PresenceStatus, PresenceUpdate};

    fn create_test_message() -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-msg-1".to_string(),
            timestamp: 12345,
            payload: MessagePayload::Presence(PresenceUpdate {
                status: PresenceStatus::Online,
                message: None,
            }),
        }
    }

    #[test]
    fn test_mock_transport_connect_disconnect() {
        let mut transport = MockTransport::new();

        assert_eq!(transport.state(), ConnectionState::Disconnected);

        transport.connect(&TransportConfig::default()).unwrap();
        assert_eq!(transport.state(), ConnectionState::Connected);

        transport.disconnect().unwrap();
        assert_eq!(transport.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_mock_transport_send_receive() {
        let mut transport = MockTransport::new();
        transport.connect(&TransportConfig::default()).unwrap();

        // Queue a message to receive
        let incoming = create_test_message();
        transport.queue_receive(incoming.clone());

        // Receive it
        let received = transport.receive().unwrap().unwrap();
        assert_eq!(received.message_id, incoming.message_id);

        // No more messages
        assert!(transport.receive().unwrap().is_none());
    }

    #[test]
    fn test_mock_transport_send_tracks_messages() {
        let mut transport = MockTransport::new();
        transport.connect(&TransportConfig::default()).unwrap();

        let message = create_test_message();
        transport.send(&message).unwrap();

        assert_eq!(transport.sent_messages().len(), 1);
        assert_eq!(transport.sent_messages()[0].message_id, message.message_id);
    }

    #[test]
    fn test_mock_transport_error_injection() {
        let mut transport = MockTransport::new();
        transport.inject_error(NetworkError::ConnectionFailed("test error".into()));

        let result = transport.connect(&TransportConfig::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }

    #[test]
    fn test_mock_transport_auto_ack() {
        let mut transport = MockTransport::new();
        transport.set_auto_ack(true);
        transport.connect(&TransportConfig::default()).unwrap();

        let message = create_test_message();
        transport.send(&message).unwrap();

        // Should have an ack in the receive queue
        assert!(transport.has_pending());
        let ack = transport.receive().unwrap().unwrap();

        if let MessagePayload::Acknowledgment(ack_payload) = ack.payload {
            assert_eq!(ack_payload.message_id, message.message_id);
            assert_eq!(ack_payload.status, AckStatus::Delivered);
        } else {
            panic!("Expected acknowledgment message");
        }
    }

    #[test]
    fn test_mock_transport_not_connected_error() {
        let mut transport = MockTransport::new();

        // Try to send without connecting
        let message = create_test_message();
        let result = transport.send(&message);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NetworkError::NotConnected));
    }

    #[test]
    fn test_mock_transport_clear_sent() {
        let mut transport = MockTransport::new();
        transport.connect(&TransportConfig::default()).unwrap();

        transport.send(&create_test_message()).unwrap();
        assert_eq!(transport.sent_messages().len(), 1);

        transport.clear_sent();
        assert!(transport.sent_messages().is_empty());
    }

    #[test]
    fn test_mock_transport_set_state() {
        let mut transport = MockTransport::new();

        transport.set_state(ConnectionState::Reconnecting { attempt: 3 });
        assert_eq!(
            transport.state(),
            ConnectionState::Reconnecting { attempt: 3 }
        );
    }

    #[test]
    fn test_mock_transport_has_pending() {
        let mut transport = MockTransport::new();
        transport.connect(&TransportConfig::default()).unwrap();

        assert!(!transport.has_pending());

        transport.queue_receive(create_test_message());
        assert!(transport.has_pending());

        transport.receive().unwrap();
        assert!(!transport.has_pending());
    }
}

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

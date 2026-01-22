//! Relay Client
//!
//! High-level interface for sending encrypted updates through the relay.

use std::collections::HashMap;
use std::time::Instant;

use super::connection::ConnectionManager;
use super::error::NetworkError;
use super::message::{
    AckStatus, EncryptedUpdate, MessageEnvelope, MessageId, MessagePayload, RatchetHeader,
};
use super::protocol::create_envelope;
use super::transport::{Transport, TransportConfig};
use crate::crypto::ratchet::{DoubleRatchetState, RatchetMessage};

/// Configuration for the relay client.
#[derive(Debug, Clone)]
pub struct RelayClientConfig {
    /// Transport configuration.
    pub transport: TransportConfig,
    /// Maximum concurrent pending messages.
    pub max_pending_messages: usize,
    /// Acknowledgment timeout in milliseconds.
    pub ack_timeout_ms: u64,
    /// Maximum message retries before giving up.
    pub max_retries: u32,
}

impl Default for RelayClientConfig {
    fn default() -> Self {
        RelayClientConfig {
            transport: TransportConfig::default(),
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 5,
        }
    }
}

/// Tracks an in-flight message awaiting acknowledgment.
#[derive(Debug)]
#[allow(dead_code)] // Fields used for tracking and future retry logic
struct InFlightMessage {
    message_id: MessageId,
    update_id: String,
    sent_at: Instant,
    retry_count: u32,
}

/// Relay client for sending encrypted updates.
///
/// Integrates with the sync system to process pending updates and handles
/// acknowledgment tracking, retries, and ordering guarantees.
///
/// # Example
///
/// ```ignore
/// use vauchi_core::network::{RelayClient, RelayClientConfig, MockTransport};
///
/// let transport = MockTransport::new();
/// let config = RelayClientConfig::default();
/// let mut client = RelayClient::new(transport, config, "my-identity-id".into());
///
/// client.connect()?;
/// let msg_id = client.send_update(recipient_id, &mut ratchet, &payload, update_id)?;
/// ```
pub struct RelayClient<T: Transport> {
    connection: ConnectionManager<T>,
    config: RelayClientConfig,
    /// Messages sent but not yet acknowledged: message_id -> tracking info
    in_flight: HashMap<MessageId, InFlightMessage>,
    /// Our identity public key fingerprint (for sender_id).
    our_identity_id: String,
}

impl<T: Transport> RelayClient<T> {
    /// Creates a new relay client.
    pub fn new(transport: T, config: RelayClientConfig, our_identity_id: String) -> Self {
        let connection = ConnectionManager::new(transport, config.transport.clone());

        RelayClient {
            connection,
            config,
            in_flight: HashMap::new(),
            our_identity_id,
        }
    }

    /// Connects to the relay server.
    pub fn connect(&mut self) -> Result<(), NetworkError> {
        self.connection.connect()
    }

    /// Disconnects from the relay server.
    pub fn disconnect(&mut self) -> Result<(), NetworkError> {
        self.connection.disconnect()
    }

    /// Returns true if connected.
    pub fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    /// Sends an encrypted update to a contact.
    ///
    /// The update is encrypted using the Double Ratchet before sending.
    /// Returns the message ID for tracking acknowledgments.
    pub fn send_update(
        &mut self,
        recipient_id: &str,
        ratchet: &mut DoubleRatchetState,
        payload: &[u8],
        update_id: &str,
    ) -> Result<MessageId, NetworkError> {
        // Check in-flight limit
        if self.in_flight.len() >= self.config.max_pending_messages {
            return Err(NetworkError::SendFailed("Too many pending messages".into()));
        }

        // Encrypt with Double Ratchet
        let ratchet_msg = ratchet
            .encrypt(payload)
            .map_err(|e| NetworkError::Encryption(e.to_string()))?;

        // Convert to wire format
        let envelope = self.create_update_envelope(recipient_id, &ratchet_msg);
        let message_id = envelope.message_id.clone();

        // Send
        self.connection.send(&envelope)?;

        // Track in-flight
        self.in_flight.insert(
            message_id.clone(),
            InFlightMessage {
                message_id: message_id.clone(),
                update_id: update_id.to_string(),
                sent_at: Instant::now(),
                retry_count: 0,
            },
        );

        Ok(message_id)
    }

    /// Sends a raw encrypted update (already encrypted externally).
    ///
    /// Use this when you've already encrypted the message and just need
    /// to send it through the relay.
    pub fn send_raw_update(
        &mut self,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
        update_id: &str,
    ) -> Result<MessageId, NetworkError> {
        if self.in_flight.len() >= self.config.max_pending_messages {
            return Err(NetworkError::SendFailed("Too many pending messages".into()));
        }

        let envelope = self.create_update_envelope(recipient_id, ratchet_msg);
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        self.in_flight.insert(
            message_id.clone(),
            InFlightMessage {
                message_id: message_id.clone(),
                update_id: update_id.to_string(),
                sent_at: Instant::now(),
                retry_count: 0,
            },
        );

        Ok(message_id)
    }

    /// Processes incoming messages (acknowledgments, updates from others).
    ///
    /// Returns a list of update IDs that have been successfully acknowledged.
    pub fn process_incoming(&mut self) -> Result<Vec<String>, NetworkError> {
        let mut acknowledged = Vec::new();

        while let Some(envelope) = self.connection.receive()? {
            match envelope.payload {
                MessagePayload::Acknowledgment(ack) => {
                    if let Some(in_flight) = self.in_flight.remove(&ack.message_id) {
                        if ack.status == AckStatus::Stored
                            || ack.status == AckStatus::Delivered
                            || ack.status == AckStatus::ReceivedByRecipient
                        {
                            acknowledged.push(in_flight.update_id);
                        }
                        // For Failed status, the message stays removed but not acknowledged
                        // The caller should handle retry logic
                    }
                }
                MessagePayload::EncryptedUpdate(_) => {
                    // Incoming updates from others - to be handled by application layer
                    // Could emit via callback or store for later retrieval
                }
                _ => {
                    // Ignore other message types
                }
            }
        }

        Ok(acknowledged)
    }

    /// Checks for timed-out messages and returns their update IDs.
    ///
    /// Timed-out messages are removed from the in-flight tracking.
    /// The caller should handle retry logic.
    pub fn check_timeouts(&mut self) -> Vec<String> {
        let now = Instant::now();
        let timeout = std::time::Duration::from_millis(self.config.ack_timeout_ms);

        let timed_out: Vec<_> = self
            .in_flight
            .iter()
            .filter(|(_, msg)| now.duration_since(msg.sent_at) > timeout)
            .map(|(id, msg)| (id.clone(), msg.update_id.clone()))
            .collect();

        for (msg_id, _) in &timed_out {
            self.in_flight.remove(msg_id);
        }

        timed_out
            .into_iter()
            .map(|(_, update_id)| update_id)
            .collect()
    }

    /// Returns the number of in-flight messages.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Returns true if there are in-flight messages.
    pub fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// Returns the update IDs of all in-flight messages.
    pub fn in_flight_update_ids(&self) -> Vec<String> {
        self.in_flight
            .values()
            .map(|m| m.update_id.clone())
            .collect()
    }

    /// Returns a reference to the connection manager.
    pub fn connection(&self) -> &ConnectionManager<T> {
        &self.connection
    }

    /// Returns a mutable reference to the connection manager.
    pub fn connection_mut(&mut self) -> &mut ConnectionManager<T> {
        &mut self.connection
    }

    /// Creates an encrypted update envelope from a ratchet message.
    fn create_update_envelope(
        &self,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
    ) -> MessageEnvelope {
        let encrypted_update = EncryptedUpdate {
            recipient_id: recipient_id.to_string(),
            sender_id: self.our_identity_id.clone(),
            ratchet_header: RatchetHeader {
                dh_public: ratchet_msg.dh_public,
                dh_generation: ratchet_msg.dh_generation,
                message_index: ratchet_msg.message_index,
                previous_chain_length: ratchet_msg.previous_chain_length,
            },
            ciphertext: ratchet_msg.ciphertext.clone(),
        };

        create_envelope(MessagePayload::EncryptedUpdate(encrypted_update))
    }
}

/// Result of processing pending updates.
#[derive(Debug, Default)]
pub struct ProcessResult {
    /// Number of updates sent.
    pub sent: usize,
    /// Number of updates acknowledged.
    pub acknowledged: usize,
    /// Number of updates skipped (no ratchet available).
    pub skipped: usize,
    /// Number of sends that failed.
    pub failed: usize,
    /// Message IDs of sent messages.
    pub message_ids: Vec<MessageId>,
    /// Errors encountered.
    pub errors: Vec<(String, NetworkError)>,
}

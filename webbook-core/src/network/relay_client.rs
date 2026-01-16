//! Relay Client
//!
//! High-level interface for sending encrypted updates through the relay.

use std::collections::HashMap;
use std::time::Instant;

use super::connection::ConnectionManager;
use super::error::NetworkError;
use super::message::{
    AckStatus, EncryptedUpdate, MessageEnvelope,
    MessageId, MessagePayload, RatchetHeader,
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
#[allow(dead_code)]  // Fields used for tracking and future retry logic
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
/// use webbook_core::network::{RelayClient, RelayClientConfig, MockTransport};
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
    pub fn new(
        transport: T,
        config: RelayClientConfig,
        our_identity_id: String,
    ) -> Self {
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
            return Err(NetworkError::SendFailed(
                "Too many pending messages".into()
            ));
        }

        // Encrypt with Double Ratchet
        let ratchet_msg = ratchet.encrypt(payload)
            .map_err(|e| NetworkError::Encryption(e.to_string()))?;

        // Convert to wire format
        let envelope = self.create_update_envelope(recipient_id, &ratchet_msg);
        let message_id = envelope.message_id.clone();

        // Send
        self.connection.send(&envelope)?;

        // Track in-flight
        self.in_flight.insert(message_id.clone(), InFlightMessage {
            message_id: message_id.clone(),
            update_id: update_id.to_string(),
            sent_at: Instant::now(),
            retry_count: 0,
        });

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
            return Err(NetworkError::SendFailed(
                "Too many pending messages".into()
            ));
        }

        let envelope = self.create_update_envelope(recipient_id, ratchet_msg);
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        self.in_flight.insert(message_id.clone(), InFlightMessage {
            message_id: message_id.clone(),
            update_id: update_id.to_string(),
            sent_at: Instant::now(),
            retry_count: 0,
        });

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
                        if ack.status == AckStatus::Delivered
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

        let timed_out: Vec<_> = self.in_flight
            .iter()
            .filter(|(_, msg)| now.duration_since(msg.sent_at) > timeout)
            .map(|(id, msg)| (id.clone(), msg.update_id.clone()))
            .collect();

        for (msg_id, _) in &timed_out {
            self.in_flight.remove(msg_id);
        }

        timed_out.into_iter().map(|(_, update_id)| update_id).collect()
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
        self.in_flight.values()
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
    fn create_update_envelope(&self, recipient_id: &str, ratchet_msg: &RatchetMessage) -> MessageEnvelope {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;
    use crate::network::mock::MockTransport;
    use crate::exchange::X3DHKeyPair;

    fn create_test_config() -> RelayClientConfig {
        RelayClientConfig {
            transport: TransportConfig::default(),
            max_pending_messages: 10,
            ack_timeout_ms: 100, // Short timeout for testing
            max_retries: 3,
        }
    }

    fn create_test_ratchet() -> (DoubleRatchetState, DoubleRatchetState) {
        let _alice_dh = X3DHKeyPair::generate();
        let bob_dh = X3DHKeyPair::generate();
        let shared_secret = SymmetricKey::generate();

        let alice = DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
        let bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        (alice, bob)
    }

    #[test]
    fn test_relay_client_connect_disconnect() {
        let transport = MockTransport::new();
        let mut client = RelayClient::new(transport, create_test_config(), "test-id".into());

        assert!(!client.is_connected());

        client.connect().unwrap();
        assert!(client.is_connected());

        client.disconnect().unwrap();
        assert!(!client.is_connected());
    }

    #[test]
    fn test_relay_client_send_update() {
        let transport = MockTransport::new();
        let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _bob_ratchet) = create_test_ratchet();
        let payload = b"Hello, Bob!";

        let msg_id = client.send_update(
            "recipient-id",
            &mut alice_ratchet,
            payload,
            "update-1",
        ).unwrap();

        assert!(!msg_id.is_empty());
        assert_eq!(client.in_flight_count(), 1);

        // Check the message was sent
        let sent = client.connection().transport().sent_messages();
        assert_eq!(sent.len(), 1);

        if let MessagePayload::EncryptedUpdate(update) = &sent[0].payload {
            assert_eq!(update.recipient_id, "recipient-id");
            assert_eq!(update.sender_id, "sender-id");
        } else {
            panic!("Expected EncryptedUpdate");
        }
    }

    #[test]
    fn test_relay_client_acknowledgment_tracking() {
        let mut transport = MockTransport::new();
        transport.set_auto_ack(true);

        let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _) = create_test_ratchet();

        // Send a message
        let _msg_id = client.send_update(
            "recipient-id",
            &mut alice_ratchet,
            b"test",
            "update-1",
        ).unwrap();

        assert_eq!(client.in_flight_count(), 1);

        // Process ack
        let acked = client.process_incoming().unwrap();

        assert_eq!(acked.len(), 1);
        assert_eq!(acked[0], "update-1");
        assert_eq!(client.in_flight_count(), 0);
    }

    #[test]
    fn test_relay_client_timeout_detection() {
        let transport = MockTransport::new();
        let mut config = create_test_config();
        config.ack_timeout_ms = 1; // Very short timeout

        let mut client = RelayClient::new(transport, config, "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _) = create_test_ratchet();

        // Send a message
        client.send_update(
            "recipient-id",
            &mut alice_ratchet,
            b"test",
            "update-1",
        ).unwrap();

        // Wait for timeout
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Check timeouts
        let timed_out = client.check_timeouts();

        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], "update-1");
        assert_eq!(client.in_flight_count(), 0);
    }

    #[test]
    fn test_relay_client_max_pending_limit() {
        let transport = MockTransport::new();
        let mut config = create_test_config();
        config.max_pending_messages = 2;

        let mut client = RelayClient::new(transport, config, "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _) = create_test_ratchet();

        // Send up to limit
        client.send_update("r1", &mut alice_ratchet, b"1", "u1").unwrap();
        client.send_update("r2", &mut alice_ratchet, b"2", "u2").unwrap();

        // Third should fail
        let result = client.send_update("r3", &mut alice_ratchet, b"3", "u3");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many pending"));
    }

    #[test]
    fn test_relay_client_in_flight_update_ids() {
        let transport = MockTransport::new();
        let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _) = create_test_ratchet();

        client.send_update("r1", &mut alice_ratchet, b"1", "update-a").unwrap();
        client.send_update("r2", &mut alice_ratchet, b"2", "update-b").unwrap();

        let ids = client.in_flight_update_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"update-a".to_string()));
        assert!(ids.contains(&"update-b".to_string()));
    }

    #[test]
    fn test_relay_client_has_in_flight() {
        let transport = MockTransport::new();
        let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
        client.connect().unwrap();

        assert!(!client.has_in_flight());

        let (mut alice_ratchet, _) = create_test_ratchet();
        client.send_update("r1", &mut alice_ratchet, b"1", "u1").unwrap();

        assert!(client.has_in_flight());
    }

    #[test]
    fn test_relay_client_send_raw_update() {
        let transport = MockTransport::new();
        let mut client = RelayClient::new(transport, create_test_config(), "sender-id".into());
        client.connect().unwrap();

        let (mut alice_ratchet, _) = create_test_ratchet();

        // Encrypt externally
        let ratchet_msg = alice_ratchet.encrypt(b"raw message").unwrap();

        // Send raw
        let msg_id = client.send_raw_update("recipient-id", &ratchet_msg, "raw-update-1").unwrap();

        assert!(!msg_id.is_empty());
        assert_eq!(client.in_flight_count(), 1);
    }

    #[test]
    fn test_process_result_default() {
        let result = ProcessResult::default();
        assert_eq!(result.sent, 0);
        assert_eq!(result.acknowledged, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.failed, 0);
        assert!(result.message_ids.is_empty());
        assert!(result.errors.is_empty());
    }
}

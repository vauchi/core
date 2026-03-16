// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay Client
//!
//! High-level interface for sending encrypted updates through the relay.

use std::collections::HashMap;
use std::time::Instant;

use super::connection::ConnectionManager;
use super::error::NetworkError;
use super::message::{
    AckStatus, DeviceSyncMessage, EncryptedUpdate, MessageEnvelope, MessageId, MessagePayload,
    PurgeRequest, RatchetHeader,
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
    /// Whether to send delivery receipts for received messages.
    /// When false, the client will not send ReceivedByRecipient ACKs.
    pub delivery_receipts_enabled: bool,
    /// Whether to suppress presence (online/offline status) at the relay.
    /// When true, the relay will not notify contacts of this client's online status.
    pub suppress_presence: bool,
}

impl Default for RelayClientConfig {
    fn default() -> Self {
        RelayClientConfig {
            transport: TransportConfig::default(),
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 5,
            delivery_receipts_enabled: true,
            suppress_presence: false,
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
        let mut connection = ConnectionManager::new(transport, config.transport.clone());
        connection.set_suppress_presence(config.suppress_presence);

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

    /// Sends a device sync message to another device.
    ///
    /// Used for syncing data between devices belonging to the same identity.
    /// The ciphertext should already be encrypted for the target device.
    pub fn send_device_sync_message(
        &mut self,
        sender_device_id: &[u8; 32],
        target_device_id: &[u8; 32],
        ciphertext: Vec<u8>,
        nonce: [u8; 12],
        sync_version: u64,
    ) -> Result<MessageId, NetworkError> {
        let sync_msg = DeviceSyncMessage {
            target_device_id: *target_device_id,
            sender_device_id: *sender_device_id,
            ciphertext,
            nonce,
            sync_version,
        };

        let envelope = create_envelope(MessagePayload::DeviceSync(sync_msg));
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        Ok(message_id)
    }

    /// Sends a purge request to the relay server.
    ///
    /// Requests the relay to delete all stored messages and data for this identity.
    /// Used during account shredding (hard_shred / panic_shred).
    pub fn send_purge_request(
        &mut self,
        request: &PurgeRequest,
    ) -> Result<MessageId, NetworkError> {
        let envelope = create_envelope(MessagePayload::PurgeRequest(request.clone()));
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        Ok(message_id)
    }

    /// Sends an account revocation message to a contact via the relay.
    ///
    /// Used during identity deletion (hard_shred / panic_shred) to notify
    /// contacts that this identity has been revoked. The message is signed
    /// (not encrypted) so it can be processed even without ratchet state.
    pub fn send_account_revoked(
        &mut self,
        revoked: &super::message::AccountRevoked,
    ) -> Result<MessageId, NetworkError> {
        let envelope = create_envelope(MessagePayload::AccountRevoked(revoked.clone()));
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        Ok(message_id)
    }

    /// Processes incoming messages (acknowledgments, updates from others).
    ///
    /// Returns an `IncomingResult` containing:
    /// - `acknowledged`: Update IDs that were successfully acknowledged
    /// - `ack_events`: All ACK events including failures, for delivery tracking
    pub fn process_incoming(&mut self) -> Result<IncomingResult, NetworkError> {
        let mut result = IncomingResult::default();

        while let Some(envelope) = self.connection.receive()? {
            match envelope.payload {
                MessagePayload::Acknowledgment(ack) => {
                    if let Some(in_flight) = self.in_flight.remove(&ack.message_id) {
                        // Record ACK event for delivery tracking
                        result.ack_events.push(AckEvent {
                            update_id: in_flight.update_id.clone(),
                            status: ack.status,
                            error: ack.error.clone(),
                        });

                        if ack.status == AckStatus::Stored
                            || ack.status == AckStatus::Delivered
                            || ack.status == AckStatus::ReceivedByRecipient
                        {
                            result.acknowledged.push(in_flight.update_id);
                        }
                    }
                }
                MessagePayload::EncryptedUpdate(_) => {
                    // Incoming updates from others - to be handled by application layer
                }
                _ => {
                    // Ignore other message types
                }
            }
        }

        Ok(result)
    }

    /// Checks for timed-out messages and returns their update IDs.
    ///
    /// Timed-out messages are removed from the in-flight tracking.
    /// The caller should handle retry logic.
    pub fn check_timeouts(&mut self) -> Vec<String> {
        self.check_timeouts_at(Instant::now())
    }

    /// Checks for timed-out messages at a given point in time.
    pub fn check_timeouts_at(&mut self, now: Instant) -> Vec<String> {
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

    /// Returns a reference to the client configuration.
    pub fn config(&self) -> &RelayClientConfig {
        &self.config
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

impl<T: Transport> crate::api::PurgeSender for RelayClient<T> {
    fn send_purge(
        &mut self,
        purge: &crate::api::PreSignedPurgeRequest,
    ) -> Result<bool, crate::api::ShredError> {
        let request = PurgeRequest {
            public_key: purge.public_key,
            signature: purge.signature.clone(),
            purge_token: purge.purge_token,
            timestamp: purge.timestamp,
        };

        match self.send_purge_request(&request) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay purge failed: {}",
                e
            ))),
        }
    }
}

impl<T: Transport> crate::api::RevocationSender for RelayClient<T> {
    fn send_revocation(
        &mut self,
        revocation: &crate::network::AccountRevoked,
    ) -> Result<bool, crate::api::ShredError> {
        match self.send_account_revoked(revocation) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay revocation failed: {}",
                e
            ))),
        }
    }
}

/// An ACK event received from the relay.
///
/// Captures the full acknowledgment status for delivery tracking,
/// including failed ACKs that were previously silently dropped.
#[derive(Debug, Clone)]
pub struct AckEvent {
    /// The application-level update ID (PendingUpdate.id / DeliveryRecord.message_id).
    pub update_id: String,
    /// The ACK status from the relay.
    pub status: AckStatus,
    /// Optional error message (for Failed status).
    pub error: Option<String>,
}

/// Result of processing incoming messages from the relay.
#[derive(Debug, Default)]
pub struct IncomingResult {
    /// Update IDs that were successfully acknowledged (Stored/Delivered/ReceivedByRecipient).
    pub acknowledged: Vec<String>,
    /// All ACK events including failures, for delivery tracking.
    pub ack_events: Vec<AckEvent>,
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

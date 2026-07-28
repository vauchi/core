// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay Client
//!
//! High-level interface for sending encrypted updates through the relay.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::connection::ConnectionManager;
use super::error::NetworkError;
use super::mailbox_token::{
    batch_register_tokens, batch_register_tokens_with_device_sync, compute_device_sync_token,
    current_day_epoch, token_hex,
};
use super::message::{
    AckStatus, EncryptedUpdate, MessageEnvelope, MessageId, MessagePayload, PurgeRequest,
    RatchetHeader,
};
use super::protocol::create_envelope;
use super::transport::{Transport, TransportConfig};
use crate::crypto::ratchet::{DoubleRatchetState, RatchetMessage};
use crate::identifiers::ContactId;
use crate::identity::Identity;
use crate::monotonic::{MonotonicClock, SystemMonotonicClock};
use crate::rng::SecureRngExt;

/// Generates a hex-encoded anonymous sender ID from an optional shared key.
/// Returns `None` if no shared key is provided (backward compat — uses real identity).
fn anonymous_sender_hex(shared_key: Option<&[u8; 32]>, now: u64) -> Option<String> {
    shared_key.map(|key| {
        let anon = super::anonymous::AnonymousSender::for_current_epoch(key, now);
        hex::encode(anon.anonymous_id)
    })
}

/// Generates the rotating sender token scoped to this concrete sending device.
fn anonymous_sender_hex_for_device(
    shared_key: Option<&[u8; 32]>,
    sender_device_id: Option<&[u8; 32]>,
    now: u64,
) -> Option<String> {
    match (shared_key, sender_device_id) {
        (Some(key), Some(device_id)) => Some(hex::encode(
            super::anonymous::compute_anonymous_id_for_device(
                key,
                super::anonymous::current_epoch(now),
                device_id,
            ),
        )),
        _ => anonymous_sender_hex(shared_key, now),
    }
}

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
    /// Explicit-monotonic-time seam (Phase 1 / Task 1.1b). Drives
    /// `in_flight` `sent_at` stamps and the `check_timeouts()` deadline.
    /// Defaults to `SystemMonotonicClock::shared()`; the production
    /// `Vauchi` sync path injects its shared clock via
    /// [`RelayClient::with_monotonic`] so ack timeouts are deterministic
    /// under test.
    monotonic: Arc<dyn MonotonicClock>,
    /// Explicit RNG seam for message ID generation (C13).
    /// Defaults to `OsSecureRng::shared()`; tests inject a deterministic
    /// rng via [`RelayClient::with_rng`].
    rng: Arc<dyn crate::rng::SecureRng>,
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
            monotonic: SystemMonotonicClock::shared(),
            rng: crate::rng::OsSecureRng::shared(),
        }
    }

    /// Replace the [`SecureRng`] driving message ID generation.
    /// Default is [`OsSecureRng::shared`]; tests inject a deterministic
    /// rng via this builder.
    #[must_use]
    pub fn with_rng(mut self, rng: Arc<dyn crate::rng::SecureRng>) -> Self {
        self.rng = rng;
        self
    }

    /// Replace the [`MonotonicClock`] driving in-flight ack timeouts.
    /// Default is [`SystemMonotonicClock::shared`]; tests (or the
    /// `Vauchi` sync path) inject a shared clock for determinism.
    #[must_use]
    pub fn with_monotonic(mut self, monotonic: Arc<dyn MonotonicClock>) -> Self {
        self.monotonic = monotonic;
        self
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
    /// When `shared_key` is provided, the sender_id field is replaced with
    /// a rotating anonymous ID derived from the shared key, preventing
    /// relay-side correlation of sender identity across messages.
    /// Returns the message ID for tracking acknowledgments.
    pub fn send_update(
        &mut self,
        now: u64,
        recipient_id: &str,
        ratchet: &mut DoubleRatchetState,
        payload: &[u8],
        update_id: &str,
        shared_key: Option<&[u8; 32]>,
    ) -> Result<MessageId, NetworkError> {
        if self.in_flight.len() >= self.config.max_pending_messages {
            return Err(NetworkError::SendFailed("Too many pending messages".into()));
        }

        let ratchet_msg = ratchet
            .encrypt(payload)
            .map_err(|e| NetworkError::Encryption(e.to_string()))?;

        let anon_id_hex = anonymous_sender_hex(shared_key, now);
        // This legacy send path carries no sender-device id, so no origin hint.
        let envelope = self.create_update_envelope(
            recipient_id,
            &ratchet_msg,
            anon_id_hex.as_deref(),
            None,
            now,
        );
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        self.in_flight.insert(
            message_id.clone(),
            InFlightMessage {
                message_id: message_id.clone(),
                update_id: update_id.to_string(),
                sent_at: self.monotonic.now(),
            },
        );

        Ok(message_id)
    }

    /// Sends a raw encrypted update (already encrypted externally).
    ///
    /// Use this when you've already encrypted the message and just need
    /// to send it through the relay. Pass `shared_key` for anonymous sending.
    pub fn send_raw_update(
        &mut self,
        now: u64,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
        update_id: &str,
        shared_key: Option<&[u8; 32]>,
    ) -> Result<MessageId, NetworkError> {
        self.send_raw_update_for_device(now, recipient_id, ratchet_msg, update_id, shared_key, None)
    }

    /// Sends a pre-encrypted update with a sender token scoped to this device.
    pub fn send_raw_update_for_device(
        &mut self,
        now: u64,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
        update_id: &str,
        shared_key: Option<&[u8; 32]>,
        sender_device_id: Option<&[u8; 32]>,
    ) -> Result<MessageId, NetworkError> {
        self.send_raw_update_with_routing(
            now,
            recipient_id,
            ratchet_msg,
            update_id,
            shared_key,
            sender_device_id,
            sender_device_id,
        )
    }

    /// Sends a pre-encrypted update with independently selected anonymous
    /// sender-token and authenticated origin-device scopes.
    #[allow(clippy::too_many_arguments)]
    pub fn send_raw_update_with_routing(
        &mut self,
        now: u64,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
        update_id: &str,
        shared_key: Option<&[u8; 32]>,
        sender_device_id: Option<&[u8; 32]>,
        origin_device_id: Option<&[u8; 32]>,
    ) -> Result<MessageId, NetworkError> {
        if self.in_flight.len() >= self.config.max_pending_messages {
            return Err(NetworkError::SendFailed("Too many pending messages".into()));
        }

        let anon_id_hex = anonymous_sender_hex_for_device(shared_key, sender_device_id, now);
        // Stamp an origin-device hint so the receiver can select this sender
        // device's ratchet before decrypting (the HTTP transport otherwise
        // drops the sender identity). Bound to the mailbox token and the exact
        // ciphertext (F4 origin-device hint design).
        let origin_hint = match (shared_key, origin_device_id) {
            (Some(key), Some(device_id)) => {
                let ciphertext = serde_json::to_vec(ratchet_msg)
                    .expect("RatchetMessage serialization is infallible");
                crate::network::origin_hint::seal_origin_hint(
                    key,
                    device_id,
                    recipient_id,
                    &ciphertext,
                    crate::network::origin_hint::SCOPE_CARD_DELTA,
                    self.rng.as_ref(),
                )
            }
            _ => None,
        };
        let envelope = self.create_update_envelope(
            recipient_id,
            ratchet_msg,
            anon_id_hex.as_deref(),
            origin_hint,
            now,
        );
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        self.in_flight.insert(
            message_id.clone(),
            InFlightMessage {
                message_id: message_id.clone(),
                update_id: update_id.to_string(),
                sent_at: self.monotonic.now(),
            },
        );

        Ok(message_id)
    }

    /// Registers mailbox tokens with the relay for message delivery routing.
    ///
    /// Sends one `RegisterMailbox` message per 256-token batch. Most users
    /// send exactly one message; users with many contacts or long offline
    /// periods send 2–3. Each batch is padded to 256 and shuffled so the
    /// relay cannot infer the number of real contacts or detect which tokens
    /// persist across sessions. Returns the last sent message ID.
    ///
    /// SP-33 Task 4.2.
    pub fn register_mailbox_tokens(
        &mut self,
        contact_keys: &[[u8; 32]],
        own_pubkey: &[u8; 32],
        master_seed: &[u8; 32],
        days_offline: u64,
        now: u64,
        rng: &dyn crate::rng::SecureRng,
    ) -> Result<MessageId, NetworkError> {
        let day = current_day_epoch(now);
        let batches = batch_register_tokens(
            rng,
            contact_keys,
            own_pubkey,
            master_seed,
            day,
            days_offline,
        );
        self.send_mailbox_registration_batches(batches, now)
    }

    /// Registers contact mailboxes plus this device's recipient-specific sync
    /// mailbox. Use this for a linked-device sync cycle: relays may replace a
    /// prior registration set, so the send-phase registration must not drop
    /// the receive mailbox registered before fetch.
    pub fn register_mailbox_tokens_with_device_sync(
        &mut self,
        contact_keys: &[[u8; 32]],
        identity: &Identity,
        days_offline: u64,
        now: u64,
        rng: &dyn crate::rng::SecureRng,
    ) -> Result<MessageId, NetworkError> {
        let day = current_day_epoch(now);
        let batches = batch_register_tokens_with_device_sync(
            rng,
            contact_keys,
            identity.signing_public_key(),
            identity.master_seed(),
            identity.device_id(),
            day,
            days_offline,
        );
        self.send_mailbox_registration_batches(batches, now)
    }

    fn send_mailbox_registration_batches(
        &mut self,
        batches: Vec<Vec<String>>,
        now: u64,
    ) -> Result<MessageId, NetworkError> {
        let mut last_message_id = MessageId::from(String::new());
        for tokens in batches {
            let message_id: MessageId = self.rng.uuid_v4().into();
            let envelope = create_envelope(
                MessagePayload::RegisterMailbox(super::message::RegisterMailbox { tokens }),
                now,
                message_id.clone(),
            );
            last_message_id = message_id;
            self.connection.send(&envelope)?;
        }

        Ok(last_message_id)
    }

    /// Sends a device sync message via self-token EncryptedUpdate.
    ///
    /// Wraps the encrypted sync payload in an `EncryptedUpdate` where
    /// `recipient_id` is the daily self-token, so all devices sharing the
    /// same master seed receive it.
    ///
    /// SP-33 Task 4.3.
    pub fn send_device_sync_message(
        &mut self,
        master_seed: &[u8; 32],
        target_device_id: &[u8; 32],
        ciphertext: Vec<u8>,
        now: u64,
    ) -> Result<MessageId, NetworkError> {
        let device_token =
            compute_device_sync_token(master_seed, target_device_id, current_day_epoch(now));

        // Device sync carries no Double Ratchet: `ciphertext` is already
        // sealed by `DeviceSyncOrchestrator::encrypt_for_device` (ECDH from
        // the shared master seed + HKDF + XChaCha20-Poly1305). The wire
        // `EncryptedUpdate` requires a `RatchetHeader`, so device-sync blobs
        // carry a synthetic zero header that the device-sync receive path
        // ignores — it decrypts via `decrypt_from_device`, not the ratchet.
        let encrypted_update = EncryptedUpdate {
            recipient_id: ContactId::from(token_hex(&device_token)),
            sender_id: ContactId::from(self.our_identity_id.clone()),
            ratchet_header: RatchetHeader {
                dh_public: crate::identifiers::DhPublicKey::from([0u8; 32]),
                dh_generation: 0,
                message_index: 0,
                previous_chain_length: 0,
            },
            ciphertext,
            // Device-sync and revocation traffic is not a device-pair card
            // delta, so it carries no origin hint.
            origin_hint: None,
        };

        let message_id: MessageId = self.rng.uuid_v4().into();
        let envelope = create_envelope(
            MessagePayload::EncryptedUpdate(encrypted_update),
            now,
            message_id.clone(),
        );
        let message_id = envelope.message_id.clone();

        self.connection.send(&envelope)?;

        Ok(message_id)
    }

    /// Sends a purge request to the relay server.
    ///
    /// Requests the relay to delete all stored messages and data for this identity.
    /// Used during identity shredding (hard_shred / panic_shred).
    pub fn send_purge_request(
        &mut self,
        request: &PurgeRequest,
        now: u64,
    ) -> Result<MessageId, NetworkError> {
        let message_id: MessageId = self.rng.uuid_v4().into();
        let envelope = create_envelope(
            MessagePayload::PurgeRequest(request.clone()),
            now,
            message_id.clone(),
        );
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
        self.check_timeouts_at(self.monotonic.now())
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
    ///
    /// When `anonymous_sender_id` is `Some`, uses it as the sender_id field
    /// (preventing relay-side correlation). When `None`, uses the real
    /// identity fingerprint (backward compat for device sync, purge, etc.).
    fn create_update_envelope(
        &self,
        recipient_id: &str,
        ratchet_msg: &RatchetMessage,
        anonymous_sender_id: Option<&str>,
        origin_hint: Option<String>,
        now: u64,
    ) -> MessageEnvelope {
        let encrypted_update = EncryptedUpdate {
            recipient_id: ContactId::from(recipient_id),
            sender_id: ContactId::from(
                anonymous_sender_id
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| self.our_identity_id.clone()),
            ),
            origin_hint,
            ratchet_header: RatchetHeader {
                dh_public: crate::identifiers::DhPublicKey::from(ratchet_msg.dh_public),
                dh_generation: ratchet_msg.dh_generation,
                message_index: ratchet_msg.message_index,
                previous_chain_length: ratchet_msg.previous_chain_length,
            },
            // The full serialized RatchetMessage, NOT the bare AEAD body: the
            // HTTP transport ships only this `ciphertext` (it drops
            // `ratchet_header`), and the receiver reconstructs via
            // `from_slice::<RatchetMessage>`. A bare body loses the DH header,
            // so the responder cannot DH-step the initiator's first message
            // (2026-06-28-sync-delivery-sent-not-received). to_vec on this
            // fixed-shape struct is infallible (cf. session.rs commitment).
            ciphertext: serde_json::to_vec(ratchet_msg)
                .expect("RatchetMessage serialization is infallible"),
        };

        let message_id: MessageId = self.rng.uuid_v4().into();
        create_envelope(
            MessagePayload::EncryptedUpdate(encrypted_update),
            now,
            message_id,
        )
    }
}

impl<T: Transport> crate::api::PurgeSender for RelayClient<T> {
    fn send_purge(
        &mut self,
        purge: &crate::api::PreSignedPurgeRequest,
        now: u64,
    ) -> Result<bool, crate::api::ShredError> {
        let request = PurgeRequest {
            public_key: crate::identifiers::IdentityKey::from(purge.public_key),
            signature: purge.signature.clone(),
            purge_token: purge.purge_token,
            timestamp: purge.timestamp,
        };

        match self.send_purge_request(&request, now) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay purge failed: {}",
                e
            ))),
        }
    }
}

impl<T: Transport> RelayClient<T> {
    /// Sends a pre-built revocation blob to a contact's mailbox token as an
    /// ordinary `EncryptedUpdate`. Unlike a bare `IdentityRevoked` envelope
    /// (which the HTTP transport drops), an `EncryptedUpdate` is delivered via
    /// the relay `send` endpoint; the recipient detects the `VRV1` magic in
    /// the ciphertext and routes it to `process_revocation`.
    pub fn send_revocation_blob(
        &mut self,
        token: &str,
        ciphertext: Vec<u8>,
        now: u64,
    ) -> Result<MessageId, NetworkError> {
        let encrypted_update = EncryptedUpdate {
            recipient_id: ContactId::from(token.to_string()),
            sender_id: ContactId::from(self.our_identity_id.clone()),
            ratchet_header: RatchetHeader {
                dh_public: crate::identifiers::DhPublicKey::from([0u8; 32]),
                dh_generation: 0,
                message_index: 0,
                previous_chain_length: 0,
            },
            ciphertext,
            // Device-sync and revocation traffic is not a device-pair card
            // delta, so it carries no origin hint.
            origin_hint: None,
        };
        let message_id: MessageId = self.rng.uuid_v4().into();
        let envelope = create_envelope(
            MessagePayload::EncryptedUpdate(encrypted_update),
            now,
            message_id.clone(),
        );
        let message_id = envelope.message_id.clone();
        self.connection.send(&envelope)?;
        Ok(message_id)
    }
}

impl<T: Transport> crate::api::RevocationSender for RelayClient<T> {
    fn send_revocation_delivery(
        &mut self,
        token: &str,
        blob_b64: &str,
        now: u64,
    ) -> Result<bool, crate::api::ShredError> {
        use base64::Engine;
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(blob_b64)
            .map_err(|e| {
                crate::api::ShredError::FileError(format!("invalid revocation blob: {e}"))
            })?;
        match self.send_revocation_blob(token, ciphertext, now) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay revocation failed: {e}"
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

// INLINE_TEST_REQUIRED: `create_update_envelope` is a private method on
// RelayClient; asserting the wire `ciphertext` contract needs same-module
// access. tests/it/ cannot reach it.
#[cfg(test)]
mod wire_format_tests {
    use super::*;
    use crate::crypto::ratchet::RatchetMessage;
    use crate::network::mock::MockTransport;

    // The HTTP transport ships only `EncryptedUpdate.ciphertext`
    // (`http_adapter::send` base64s that field and drops `ratchet_header`),
    // and the receiver reconstructs via
    // `from_slice::<RatchetMessage>(ciphertext)`. So the wire `ciphertext`
    // MUST be the FULL serialized RatchetMessage, not the bare AEAD body —
    // otherwise the Double Ratchet header (dh_public/generation/indices) is
    // lost and the responder can never DH-step the initiator's first message.
    // Regression for 2026-06-28-sync-delivery-sent-not-received (on hardware:
    // `blobsFetched=1 rejected=1 cardsUpdated=0`).
    // @internal
    #[test]
    fn update_envelope_ciphertext_is_full_serialized_ratchet_message() {
        let client = RelayClient::new(
            MockTransport::new(),
            RelayClientConfig::default(),
            "sender-identity".to_string(),
        );
        let original = RatchetMessage {
            dh_public: [9u8; 32],
            dh_generation: 4,
            message_index: 7,
            previous_chain_length: 3,
            ciphertext: b"aead-body".to_vec(),
        };
        let envelope = client.create_update_envelope("recipient-token", &original, None, None, 0);
        let MessagePayload::EncryptedUpdate(update) = envelope.payload else {
            panic!("expected EncryptedUpdate payload");
        };
        let reconstructed: RatchetMessage = serde_json::from_slice(&update.ciphertext)
            .expect("wire ciphertext must be a full serialized RatchetMessage (header preserved)");
        assert_eq!(
            reconstructed.dh_public, original.dh_public,
            "DH public must survive"
        );
        assert_eq!(reconstructed.dh_generation, original.dh_generation);
        assert_eq!(reconstructed.message_index, original.message_index);
        assert_eq!(
            reconstructed.previous_chain_length,
            original.previous_chain_length
        );
        assert_eq!(reconstructed.ciphertext, original.ciphertext);
    }
}

//! WebSocket Connection Handler
//!
//! Handles individual client connections.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, warn, error};

use crate::storage::{BlobStore, StoredBlob};
use crate::rate_limit::RateLimiter;

/// Wire protocol message types (subset of webbook-core protocol).
mod protocol {
    use serde::{Deserialize, Serialize};

    pub const PROTOCOL_VERSION: u8 = 1;
    pub const FRAME_HEADER_SIZE: usize = 4;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MessageEnvelope {
        pub version: u8,
        pub message_id: String,
        pub timestamp: u64,
        pub payload: MessagePayload,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub enum MessagePayload {
        EncryptedUpdate(EncryptedUpdate),
        Acknowledgment(Acknowledgment),
        Handshake(Handshake),
        #[serde(other)]
        Unknown,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EncryptedUpdate {
        pub recipient_id: String,
        pub sender_id: String,
        pub ciphertext: Vec<u8>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Acknowledgment {
        pub message_id: String,
        pub status: AckStatus,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum AckStatus {
        Delivered,
        ReceivedByRecipient,
        Failed,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Handshake {
        pub client_id: String,
    }

    /// Decodes a message from binary data (with length prefix).
    pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, String> {
        if data.len() < FRAME_HEADER_SIZE {
            return Err("Frame too short".to_string());
        }

        let json = &data[FRAME_HEADER_SIZE..];
        serde_json::from_slice(json).map_err(|e| e.to_string())
    }

    /// Encodes a message to binary data (with length prefix).
    pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, String> {
        let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
        let len = json.len() as u32;

        let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&json);

        Ok(frame)
    }

    /// Creates an acknowledgment envelope.
    pub fn create_ack(message_id: &str, status: AckStatus) -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: uuid::Uuid::new_v4().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            payload: MessagePayload::Acknowledgment(Acknowledgment {
                message_id: message_id.to_string(),
                status,
            }),
        }
    }

    /// Creates an encrypted update envelope for delivery.
    pub fn create_update_delivery(blob_id: &str, sender_id: &str, recipient_id: &str, data: &[u8]) -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: blob_id.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
                recipient_id: recipient_id.to_string(),
                sender_id: sender_id.to_string(),
                ciphertext: data.to_vec(),
            }),
        }
    }
}

/// Handles a WebSocket connection.
pub async fn handle_connection(
    ws_stream: WebSocketStream<TcpStream>,
    storage: Arc<dyn BlobStore>,
    rate_limiter: Arc<RateLimiter>,
    max_message_size: usize,
) {
    let (mut write, mut read) = ws_stream.split();

    // Wait for handshake to get client ID
    let client_id = match read.next().await {
        Some(Ok(Message::Binary(data))) => {
            match protocol::decode_message(&data) {
                Ok(envelope) => {
                    if let protocol::MessagePayload::Handshake(hs) = envelope.payload {
                        hs.client_id
                    } else {
                        warn!("Expected Handshake, got {:?}", envelope.payload);
                        return;
                    }
                }
                Err(e) => {
                    warn!("Failed to decode handshake: {}", e);
                    return;
                }
            }
        }
        Some(Ok(_)) => {
            warn!("Expected binary message for handshake");
            return;
        }
        Some(Err(e)) => {
            warn!("Error reading handshake: {}", e);
            return;
        }
        None => {
            debug!("Connection closed before handshake");
            return;
        }
    };

    debug!("Client identified as: {}", client_id);

    // Send any pending blobs for this client
    let pending = storage.peek(&client_id);
    for blob in pending {
        let envelope = protocol::create_update_delivery(&blob.id, &blob.sender_id, &client_id, &blob.data);
        match protocol::encode_message(&envelope) {
            Ok(data) => {
                if write.send(Message::Binary(data)).await.is_err() {
                    warn!("Failed to send pending blob to {}", client_id);
                    return;
                }
            }
            Err(e) => {
                error!("Failed to encode blob delivery: {}", e);
            }
        }
    }

    // Process incoming messages
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                // Check message size
                if data.len() > max_message_size {
                    warn!("Message too large from {}: {} bytes", client_id, data.len());
                    continue;
                }

                // Rate limit check
                if !rate_limiter.consume(&client_id) {
                    warn!("Rate limited: {}", client_id);
                    continue;
                }

                // Decode message
                let envelope = match protocol::decode_message(&data) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!("Failed to decode message from {}: {}", client_id, e);
                        continue;
                    }
                };

                match envelope.payload {
                    protocol::MessagePayload::EncryptedUpdate(update) => {
                        // Store blob for recipient
                        let blob = StoredBlob::new(update.sender_id, update.ciphertext);
                        storage.store(&update.recipient_id, blob);

                        // Send acknowledgment
                        let ack = protocol::create_ack(&envelope.message_id, protocol::AckStatus::Delivered);
                        if let Ok(ack_data) = protocol::encode_message(&ack) {
                            let _ = write.send(Message::Binary(ack_data)).await;
                        }

                        debug!("Stored blob for {}", update.recipient_id);
                    }
                    protocol::MessagePayload::Acknowledgment(ack) => {
                        // Client acknowledging receipt of a blob
                        if storage.acknowledge(&client_id, &ack.message_id) {
                            debug!("Blob {} acknowledged by {}", ack.message_id, client_id);
                        }
                    }
                    protocol::MessagePayload::Handshake(_) => {
                        // Ignore duplicate handshakes
                    }
                    protocol::MessagePayload::Unknown => {
                        debug!("Unknown message type from {}", client_id);
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = write.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                debug!("Client {} sent close", client_id);
                break;
            }
            Ok(_) => {
                // Ignore text, pong, etc.
            }
            Err(e) => {
                warn!("Error from {}: {}", client_id, e);
                break;
            }
        }
    }
}

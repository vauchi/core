// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP Transport Adapter
//!
//! Adapts the stateless [`HttpTransport`] (v2 relay API) to the stateful
//! [`Transport`] trait used by [`RelayClient`] and [`SyncController`].
//!
//! The adapter translates between the `Transport` trait's `send()`/`receive()`
//! interface and the HTTP request/response model:
//! - `send(EncryptedUpdate)` → `POST /v2/send` (via OHTTP when configured)
//! - `receive()` → `POST /v2/fetch` (polls with registered mailbox tokens)
//! - `connect()` → health check + optional OHTTP key fetch
//! - `disconnect()` → no-op (HTTP is stateless)

use std::collections::VecDeque;

use base64::Engine;

use super::error::NetworkError;
use super::http_transport::{HttpTransport, HttpTransportConfig};
use super::message::{
    EncryptedUpdate, MessageEnvelope, MessagePayload, PROTOCOL_VERSION, RatchetHeader,
};
use super::ohttp_client::OhttpClient;
use super::transport::{ConnectionState, TransportConfig, TransportResult};
use vauchi_protocol::v2::FetchedBlob;

/// Adapts [`HttpTransport`] to the [`Transport`] trait.
///
/// Holds registered mailbox tokens and a buffer of fetched-but-not-yet-returned
/// blobs. Each `receive()` call returns one buffered blob or polls the relay
/// for more.
pub struct HttpTransportAdapter {
    http: HttpTransport,
    state: ConnectionState,
    /// Mailbox tokens registered via `RegisterMailbox` messages.
    registered_tokens: Vec<String>,
    /// Buffered blobs fetched from the relay but not yet returned.
    pending_blobs: VecDeque<FetchedBlob>,
    /// Acknowledged blob IDs (sent ACK to relay on next receive cycle).
    /// Stored as (token, blob_id) where token is the first registered mailbox token.
    ack_queue: Vec<(String, String)>,
    /// Whether we've already polled the relay in this receive cycle.
    ///
    /// HTTP polling is single-shot: fetch once, drain the buffer, return None.
    /// Without this guard, a `while let Some(msg) = adapter.receive()` loop
    /// refetches the same unACKed blobs on every iteration, creating an
    /// infinite loop. Reset when new tokens are registered or on reconnect.
    has_fetched: bool,
}

impl HttpTransportAdapter {
    /// Creates a new adapter wrapping an [`HttpTransport`].
    pub fn new(http: HttpTransport) -> Self {
        Self {
            http,
            state: ConnectionState::Disconnected,
            registered_tokens: Vec::new(),
            pending_blobs: VecDeque::new(),
            ack_queue: Vec::new(),
            has_fetched: false,
        }
    }

    /// Creates an adapter from config, optionally with OHTTP encryption.
    pub fn from_config(
        config: HttpTransportConfig,
        ohttp_key: Option<Vec<u8>>,
    ) -> Result<Self, NetworkError> {
        let mut http = HttpTransport::new(config);
        if let Some(key) = ohttp_key {
            let client = OhttpClient::new(key)?;
            http.set_ohttp(client);
        }
        Ok(Self::new(http))
    }

    /// Returns whether OHTTP encryption is active.
    pub fn has_ohttp(&self) -> bool {
        self.http.has_ohttp()
    }

    /// Returns the last version policy received from the relay.
    pub fn last_version_policy(&self) -> Option<crate::version::VersionPolicy> {
        self.http.last_version_policy()
    }

    /// Set the OHTTP client for encrypted requests.
    pub fn set_ohttp(&mut self, client: OhttpClient) {
        self.http.set_ohttp(client);
    }

    /// Clear OHTTP, reverting to direct mode (if allow_direct is set).
    pub fn clear_ohttp(&mut self) {
        self.http.clear_ohttp();
    }

    /// Convert a `FetchedBlob` into a `MessageEnvelope` containing an
    /// `EncryptedUpdate`.
    fn blob_to_envelope(blob: &FetchedBlob) -> Result<MessageEnvelope, NetworkError> {
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&blob.ciphertext)
            .map_err(|e| NetworkError::Serialization(format!("invalid base64 in blob: {e}")))?;

        Ok(MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: blob.blob_id.clone(),
            timestamp: blob.created_at,
            payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
                recipient_id: String::new(), // filled by caller from token context
                sender_id: String::new(),    // opaque — relay doesn't know sender
                ratchet_header: RatchetHeader {
                    dh_public: [0u8; 32],
                    dh_generation: 0,
                    message_index: 0,
                    previous_chain_length: 0,
                },
                ciphertext,
            }),
        })
    }
}

impl super::transport::Transport for HttpTransportAdapter {
    fn connect(&mut self, _config: &TransportConfig) -> TransportResult<()> {
        self.state = ConnectionState::Connecting;
        self.has_fetched = false;
        match self.http.health_check() {
            Ok(()) => {
                self.state = ConnectionState::Connected;
                Ok(())
            }
            Err(e) => {
                self.state = ConnectionState::Disconnected;
                Err(e)
            }
        }
    }

    fn disconnect(&mut self) -> TransportResult<()> {
        self.state = ConnectionState::Disconnected;
        self.registered_tokens.clear();
        self.pending_blobs.clear();
        self.has_fetched = false;
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()> {
        match &message.payload {
            MessagePayload::EncryptedUpdate(update) => {
                let ciphertext_b64 =
                    base64::engine::general_purpose::STANDARD.encode(&update.ciphertext);
                self.http
                    .send_update(&update.recipient_id, &ciphertext_b64)?;
                Ok(())
            }
            MessagePayload::RegisterMailbox(rm) => {
                // Store tokens for polling in receive()
                let mut added = false;
                for token in &rm.tokens {
                    if !self.registered_tokens.contains(token) {
                        self.registered_tokens.push(token.clone());
                        added = true;
                    }
                }
                // New tokens registered — allow another fetch cycle
                if added {
                    self.has_fetched = false;
                }
                Ok(())
            }
            MessagePayload::Acknowledgment(ack) => {
                // Queue acknowledgment — use first registered token as recipient_id
                if let Some(token) = self.registered_tokens.first() {
                    self.ack_queue.push((token.clone(), ack.message_id.clone()));
                }
                Ok(())
            }
            MessagePayload::PurgeRequest(purge) => {
                let recipient_id = hex::encode(purge.public_key);
                let public_key = hex::encode(purge.public_key);
                let purge_token = hex::encode(purge.purge_token);
                let signature = hex::encode(&purge.signature);
                self.http.purge(
                    &recipient_id,
                    &public_key,
                    &purge_token,
                    &signature,
                    purge.timestamp,
                )?;
                Ok(())
            }
            _ => {
                // Other message types (Handshake, Presence, etc.) are
                // WebSocket-specific and don't apply to HTTP polling.
                Ok(())
            }
        }
    }

    fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        // 1. Process any queued acknowledgments
        let acks: Vec<_> = self.ack_queue.drain(..).collect();
        for (recipient_id, blob_id) in acks {
            // Best-effort ACK — don't fail the receive cycle
            let _ = self.http.acknowledge(&recipient_id, &blob_id);
        }

        // 2. Return a buffered blob if available
        if let Some(blob) = self.pending_blobs.pop_front() {
            return Self::blob_to_envelope(&blob).map(Some);
        }

        // 3. If no buffered blobs and we have tokens, poll the relay (once per cycle).
        //
        // HTTP polling is single-shot: we fetch once, drain the buffer, then
        // return None. Without this guard, callers using `while let Some(msg) =
        // adapter.receive()` refetch the same unACKed blobs on every iteration
        // because ACKs are queued (not sent inline), creating an infinite loop
        // that hangs until the process timeout.
        if self.registered_tokens.is_empty() || self.has_fetched {
            return Ok(None);
        }

        self.has_fetched = true;

        // Relay accepts at most 100 tokens per fetch request.
        // On rate-limit errors, return what we have so far rather than failing.
        const MAX_FETCH_TOKENS: usize = 100;
        for chunk in self.registered_tokens.chunks(MAX_FETCH_TOKENS) {
            match self.http.fetch(chunk) {
                Ok(blobs) => {
                    for blob in blobs {
                        self.pending_blobs.push_back(blob);
                    }
                }
                Err(NetworkError::RateLimited { .. }) if !self.pending_blobs.is_empty() => break,
                Err(e) => return Err(e),
            }
        }

        if let Some(blob) = self.pending_blobs.pop_front() {
            Self::blob_to_envelope(&blob).map(Some)
        } else {
            Ok(None)
        }
    }

    fn has_pending(&self) -> bool {
        !self.pending_blobs.is_empty()
    }
}

// INLINE_TEST_REQUIRED: tests validate the Transport trait implementation
// using the adapter with mock HTTP responses (connection-refused pattern).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::RegisterMailbox;
    use crate::network::transport::Transport;

    fn make_adapter(allow_direct: bool) -> HttpTransportAdapter {
        let config = HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(), // unreachable
            timeout_ms: 1000,
            proxy: super::super::transport::ProxyConfig::None,
            allow_direct,
        };
        HttpTransportAdapter::new(HttpTransport::new(config))
    }

    #[test]
    fn test_adapter_starts_disconnected() {
        let adapter = make_adapter(true);
        assert_eq!(adapter.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_connect_fails_when_unreachable() {
        let mut adapter = make_adapter(true);
        let result = adapter.connect(&TransportConfig::default());
        assert!(result.is_err());
        assert_eq!(adapter.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_disconnect_clears_state() {
        let mut adapter = make_adapter(true);
        adapter.registered_tokens.push("token1".into());
        adapter.disconnect().unwrap();
        assert!(adapter.registered_tokens.is_empty());
        assert_eq!(adapter.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_send_register_stores_tokens() {
        let mut adapter = make_adapter(true);
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test".to_string(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: vec!["token_a".into(), "token_b".into()],
            }),
        };
        adapter.send(&envelope).unwrap();
        assert_eq!(adapter.registered_tokens.len(), 2);
        assert!(adapter.registered_tokens.contains(&"token_a".to_string()));
    }

    #[test]
    fn test_send_register_deduplicates() {
        let mut adapter = make_adapter(true);
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test".to_string(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: vec!["token_a".into()],
            }),
        };
        adapter.send(&envelope).unwrap();
        adapter.send(&envelope).unwrap();
        assert_eq!(adapter.registered_tokens.len(), 1);
    }

    #[test]
    fn test_receive_returns_none_without_tokens() {
        let mut adapter = make_adapter(true);
        let result = adapter.receive().unwrap();
        assert!(result.is_none(), "no tokens registered = nothing to fetch");
    }

    #[test]
    fn test_receive_with_tokens_fails_unreachable() {
        let mut adapter = make_adapter(true);
        adapter.registered_tokens.push("token".into());
        let result = adapter.receive();
        assert!(result.is_err(), "unreachable relay should error");
    }

    // @internal
    #[test]
    fn test_receive_does_not_refetch_after_first_poll() {
        let mut adapter = make_adapter(true);
        adapter.registered_tokens.push("token".into());

        // First receive: tries to fetch (fails — unreachable relay)
        let r1 = adapter.receive();
        assert!(r1.is_err(), "first receive should attempt fetch and fail");
        // has_fetched is set to true BEFORE the fetch attempt
        assert!(
            adapter.has_fetched,
            "has_fetched must be true after first poll attempt"
        );

        // Second receive: should NOT refetch — returns None immediately
        let r2 = adapter.receive();
        assert!(
            r2.is_ok(),
            "second receive must not re-poll (has_fetched guard)"
        );
        assert!(
            r2.unwrap().is_none(),
            "second receive must return None after initial poll"
        );
    }

    // @internal
    #[test]
    fn test_register_new_tokens_resets_fetch_guard() {
        let mut adapter = make_adapter(true);
        adapter.registered_tokens.push("token_a".into());

        // Simulate a completed fetch cycle
        adapter.has_fetched = true;

        // Registering new tokens should reset the guard
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "reg".to_string(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: vec!["token_b".into()],
            }),
        };
        adapter.send(&envelope).unwrap();
        assert!(
            !adapter.has_fetched,
            "registering new tokens must reset has_fetched"
        );
    }

    // @internal
    #[test]
    fn test_register_duplicate_tokens_does_not_reset_fetch_guard() {
        let mut adapter = make_adapter(true);
        adapter.registered_tokens.push("token_a".into());
        adapter.has_fetched = true;

        // Registering the same token should NOT reset the guard
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "reg".to_string(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: vec!["token_a".into()],
            }),
        };
        adapter.send(&envelope).unwrap();
        assert!(
            adapter.has_fetched,
            "registering duplicate tokens must not reset has_fetched"
        );
    }

    #[test]
    fn test_has_pending_initially_false() {
        let adapter = make_adapter(true);
        assert!(!adapter.has_pending());
    }

    #[test]
    fn test_blob_to_envelope_valid() {
        let blob = FetchedBlob {
            blob_id: "blob-123".into(),
            ciphertext: base64::engine::general_purpose::STANDARD.encode(b"encrypted-data"),
            created_at: 1234567890,
        };
        let envelope = HttpTransportAdapter::blob_to_envelope(&blob).unwrap();
        assert_eq!(envelope.message_id, "blob-123");
        assert_eq!(envelope.timestamp, 1234567890);
        if let MessagePayload::EncryptedUpdate(update) = &envelope.payload {
            assert_eq!(update.ciphertext, b"encrypted-data");
        } else {
            panic!("expected EncryptedUpdate payload");
        }
    }

    #[test]
    fn test_blob_to_envelope_invalid_base64() {
        let blob = FetchedBlob {
            blob_id: "bad".into(),
            ciphertext: "not-valid-base64!!!".into(),
            created_at: 0,
        };
        let result = HttpTransportAdapter::blob_to_envelope(&blob);
        assert!(result.is_err());
    }

    #[test]
    fn test_send_encrypted_update_fails_unreachable() {
        let mut adapter = make_adapter(true);
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "msg-1".to_string(),
            timestamp: 0,
            payload: MessagePayload::EncryptedUpdate(EncryptedUpdate {
                recipient_id: "a".repeat(64),
                sender_id: "b".repeat(64),
                ratchet_header: RatchetHeader {
                    dh_public: [0u8; 32],
                    dh_generation: 0,
                    message_index: 0,
                    previous_chain_length: 0,
                },
                ciphertext: b"test".to_vec(),
            }),
        };
        let result = adapter.send(&envelope);
        assert!(result.is_err(), "unreachable relay should error on send");
    }
}

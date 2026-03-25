// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync operations for relay communication.
//!
//! This module handles sending and receiving messages through the relay,
//! including exchange messages and card updates.
//!
//! Uses async tokio-tungstenite for non-blocking WebSocket communication.
//! Storage is created in scoped blocks and dropped before `.await` points
//! to keep futures `Send` (required by UniFFI async exports).

use std::path::Path;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::{EncryptedExchangeMessage, X3DHKeyPair};
use vauchi_core::sync::process_card_updates as core_process_card_updates;
use vauchi_core::{Contact, ContactCard, Identity, Storage, SymmetricKey};

use crate::cert_pinning::{self, WsStream};
use crate::error::MobileError;
use crate::protocol::{self, AckStatus, EncryptedUpdate, ExchangeMessage, MessagePayload};
use crate::types::MobileSyncResult;
use vauchi_core::network::simple_message::create_signed_handshake;

/// Result of receiving pending messages from relay.
pub struct ReceivedMessages {
    /// Legacy plaintext exchange messages (backward compatibility).
    pub legacy_exchange: Vec<ExchangeMessage>,
    /// Encrypted exchange messages (new format).
    pub encrypted_exchange: Vec<Vec<u8>>,
    /// Card updates from existing contacts: (sender_id, ciphertext).
    pub card_updates: Vec<(String, Vec<u8>)>,
}

/// Exchange response data collected during processing for async sending.
struct ExchangeResponseData {
    recipient_id: String,
    recipient_exchange_key: [u8; 32],
}

/// Sends authenticated handshake to relay.
async fn send_handshake(
    socket: &mut WsStream,
    identity: &Identity,
    device_id: Option<&str>,
) -> Result<(), MobileError> {
    let handshake = create_signed_handshake(identity, device_id.map(|s| s.to_string()));
    let envelope = protocol::create_envelope(MessagePayload::Handshake(handshake));
    let data = protocol::encode_message(&envelope)
        .map_err(|e| MobileError::SyncFailed(format!("Encode error: {}", e)))?;
    socket
        .send(Message::Binary(data))
        .await
        .map_err(|e| MobileError::NetworkError(e.to_string()))?;
    Ok(())
}

/// Receives pending messages from relay with per-message timeout.
async fn receive_pending(socket: &mut WsStream) -> Result<ReceivedMessages, MobileError> {
    let mut legacy_exchange_messages = Vec::new();
    let mut encrypted_exchange_messages = Vec::new();
    let mut card_updates = Vec::new();
    loop {
        let msg = match tokio::time::timeout(Duration::from_secs(1), socket.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => break, // Timeout — no more pending messages
        };

        match msg {
            Message::Binary(data) => {
                if let Ok(envelope) = protocol::decode_message(&data)
                    && let MessagePayload::EncryptedUpdate(update) = envelope.payload
                {
                    classify_and_store_message(
                        update,
                        &mut legacy_exchange_messages,
                        &mut encrypted_exchange_messages,
                        &mut card_updates,
                    );

                    // Send acknowledgment
                    let ack =
                        protocol::create_ack(&envelope.message_id, AckStatus::ReceivedByRecipient);
                    if let Ok(ack_data) = protocol::encode_message(&ack) {
                        let _ = socket.send(Message::Binary(ack_data)).await;
                    }
                }
            }
            Message::Ping(data) => {
                let _ = socket.send(Message::Pong(data)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    Ok(ReceivedMessages {
        legacy_exchange: legacy_exchange_messages,
        encrypted_exchange: encrypted_exchange_messages,
        card_updates,
    })
}

/// Classifies an incoming message and stores it in the appropriate collection.
fn classify_and_store_message(
    update: EncryptedUpdate,
    legacy_exchange: &mut Vec<ExchangeMessage>,
    encrypted_exchange: &mut Vec<Vec<u8>>,
    card_updates: &mut Vec<(String, Vec<u8>)>,
) {
    // Try legacy plaintext exchange format first
    if ExchangeMessage::is_exchange(&update.ciphertext)
        && let Some(exchange) = ExchangeMessage::from_bytes(&update.ciphertext)
    {
        legacy_exchange.push(exchange);
        return;
    }

    // Try encrypted exchange format
    if EncryptedExchangeMessage::from_bytes(&update.ciphertext).is_ok() {
        encrypted_exchange.push(update.ciphertext);
        return;
    }

    // Otherwise it's a card update
    card_updates.push((update.sender_id, update.ciphertext));
}

/// Processes legacy plaintext exchange messages and creates new contacts (sync).
///
/// Returns the count added and a list of exchange responses to send asynchronously.
fn process_legacy_exchange_messages(
    identity: &Identity,
    storage: &Storage,
    messages: Vec<ExchangeMessage>,
) -> Result<(u32, Vec<ExchangeResponseData>), MobileError> {
    let mut added = 0u32;
    let mut responses = Vec::new();
    let our_x3dh = identity.x3dh_keypair();

    for exchange in messages {
        let identity_key = match parse_hex_key(&exchange.identity_public_key) {
            Some(key) => key,
            None => continue,
        };

        let public_id = hex::encode(identity_key);

        // Handle response (update contact name)
        if exchange.is_response {
            update_contact_name_if_needed(storage, &public_id, &exchange.display_name);
            continue;
        }

        // Check if contact already exists
        if storage.load_contact(&public_id)?.is_some() {
            continue;
        }

        let ephemeral_key = match parse_hex_key(&exchange.ephemeral_public_key) {
            Some(key) => key,
            None => continue,
        };

        // Perform X3DH as responder
        let shared_secret =
            match vauchi_core::exchange::X3DH::respond(&our_x3dh, &identity_key, &ephemeral_key) {
                Ok(secret) => secret,
                Err(_) => continue,
            };

        // Create and save contact
        let card = ContactCard::new(&exchange.display_name);
        let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact)?;

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        added += 1;

        // Collect response data for async sending after Storage is dropped
        responses.push(ExchangeResponseData {
            recipient_id: public_id,
            recipient_exchange_key: ephemeral_key,
        });
    }

    Ok((added, responses))
}

/// Processes encrypted exchange messages (new format with proper encryption, sync).
///
/// Returns the count added and a list of exchange responses to send asynchronously.
fn process_encrypted_exchange_messages(
    identity: &Identity,
    storage: &Storage,
    encrypted_data: Vec<Vec<u8>>,
) -> Result<(u32, Vec<ExchangeResponseData>), MobileError> {
    let mut added = 0u32;
    let mut responses = Vec::new();
    let our_x3dh = identity.x3dh_keypair();

    for data in encrypted_data {
        let encrypted_msg = match EncryptedExchangeMessage::from_bytes(&data) {
            Ok(msg) => msg,
            Err(_) => continue,
        };

        let (payload, shared_secret) = match encrypted_msg.decrypt(&our_x3dh) {
            Ok(result) => result,
            Err(_) => continue,
        };

        let public_id = hex::encode(payload.identity_key);

        // Check if contact already exists
        if storage.load_contact(&public_id)?.is_some() {
            update_contact_name_if_needed(storage, &public_id, &payload.display_name);
            continue;
        }

        // Create new contact
        let card = ContactCard::new(&payload.display_name);
        let contact = Contact::from_exchange(payload.identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact)?;

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        storage.save_ratchet_state(&contact_id, &ratchet, false)?;

        added += 1;

        // Collect response data for async sending
        responses.push(ExchangeResponseData {
            recipient_id: public_id,
            recipient_exchange_key: payload.exchange_key,
        });
    }

    Ok((added, responses))
}

/// Sends an encrypted exchange response via an already-open async socket.
async fn send_exchange_response(
    socket: &mut WsStream,
    identity: &Identity,
    recipient_id: &str,
    recipient_exchange_key: &[u8; 32],
) -> Result<(), MobileError> {
    let our_id = identity.public_id();
    let our_x3dh = identity.x3dh_keypair();
    let (encrypted_msg, _) = EncryptedExchangeMessage::create(
        &our_x3dh,
        recipient_exchange_key,
        identity.signing_public_key(),
        identity.display_name(),
    )
    .map_err(|e| MobileError::CryptoError(format!("Failed to encrypt exchange: {:?}", e)))?;

    let update = EncryptedUpdate {
        recipient_id: recipient_id.to_string(),
        sender_id: our_id,
        ciphertext: encrypted_msg
            .to_bytes()
            .map_err(|e| MobileError::CryptoError(format!("Serialization failed: {:?}", e)))?,
    };

    let envelope = protocol::create_envelope(MessagePayload::EncryptedUpdate(update));
    let data =
        protocol::encode_message(&envelope).map_err(|e| MobileError::SyncFailed(e.to_string()))?;
    socket
        .send(Message::Binary(data))
        .await
        .map_err(|e| MobileError::NetworkError(e.to_string()))?;

    Ok(())
}

/// Processes incoming card updates from existing contacts.
///
/// Delegates to core's shared secure pipeline which handles:
/// - Revoked sender rejection
/// - Blocked contact rejection
/// - Signature verification (sender + recipient binding)
/// - Replay detection via storage nonces
/// - Versioned payload handling (CEK-wrapped and legacy)
/// - Atomic transaction for ratchet + nonce + contact saves
fn process_card_updates(
    identity: &Identity,
    storage: &Storage,
    updates: Vec<(String, Vec<u8>)>,
) -> Result<(u32, Vec<String>), MobileError> {
    let result = core_process_card_updates(identity, storage, updates)?;
    Ok((result.processed, result.updated_names))
}

/// Collects pending outbound updates as serialized data for async sending.
///
/// Returns `(update_id, serialized_envelope)` pairs.
fn collect_pending_updates_data(identity: &Identity, storage: &Storage) -> Vec<(String, Vec<u8>)> {
    let contacts = match storage.list_contacts() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let our_id = identity.public_id();
    let mut result = Vec::new();

    for contact in contacts {
        let pending = match storage.get_pending_updates(contact.id()) {
            Ok(p) => p,
            Err(_) => continue,
        };

        for update in pending {
            let msg = EncryptedUpdate {
                recipient_id: contact.id().to_string(),
                sender_id: our_id.clone(),
                ciphertext: update.payload,
            };

            let envelope = protocol::create_envelope(MessagePayload::EncryptedUpdate(msg));
            if let Ok(data) = protocol::encode_message(&envelope) {
                result.push((update.id, data));
            }
        }
    }

    result
}

/// Performs a complete sync operation (async, Storage scoped for Send safety).
///
/// Storage is created in scoped blocks and dropped before any `.await` points
/// so the returned future is `Send` (required by UniFFI async exports).
pub async fn do_sync_async(
    storage_path: &Path,
    storage_key: SymmetricKey,
    identity: &Identity,
    relay_url: &str,
    pinned_cert: Option<&str>,
) -> Result<MobileSyncResult, MobileError> {
    let device_id_hex = hex::encode(identity.device_id());

    // Phase 1: Connect to relay and receive messages (async, no Storage)
    let mut socket = cert_pinning::connect_with_pinning(relay_url, pinned_cert)
        .await
        .map_err(|e| MobileError::NetworkError(e.to_string()))?;

    send_handshake(&mut socket, identity, Some(&device_id_hex)).await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let received = receive_pending(&mut socket).await?;

    // Phase 2: Process received messages (Storage scoped, no await)
    let (contacts_added, exchange_responses, cards_updated, updated_contact_names, pending_updates) = {
        let storage = Storage::open(storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        // Process legacy exchange messages
        let (legacy_added, mut responses) =
            process_legacy_exchange_messages(identity, &storage, received.legacy_exchange)?;

        // Process encrypted exchange messages
        let (encrypted_added, encrypted_responses) =
            process_encrypted_exchange_messages(identity, &storage, received.encrypted_exchange)?;
        responses.extend(encrypted_responses);

        let contacts_added = legacy_added + encrypted_added;

        // Process card updates (anonymous sender ID resolution is handled
        // internally by process_card_updates — no pre-resolution needed)
        let (cards_updated, updated_contact_names) =
            process_card_updates(identity, &storage, received.card_updates)?;

        // SP-33: device sync receive + send uses EncryptedUpdate + self-token

        // Collect pending updates
        let pending_updates = collect_pending_updates_data(identity, &storage);

        (
            contacts_added,
            responses,
            cards_updated,
            updated_contact_names,
            pending_updates,
        )
        // storage dropped here
    };

    // Phase 3: Send outbound data (async, no Storage)

    // Send exchange responses via the same connection
    for response in &exchange_responses {
        let _ = send_exchange_response(
            &mut socket,
            identity,
            &response.recipient_id,
            &response.recipient_exchange_key,
        )
        .await;
    }

    // Send pending updates
    let mut updates_sent = 0u32;
    let mut sent_ids = Vec::new();
    for (update_id, data) in pending_updates {
        if socket.send(Message::Binary(data)).await.is_ok() {
            sent_ids.push(update_id);
            updates_sent += 1;
        }
    }

    // Close connection
    let _ = socket.close(None).await;

    // Phase 4: Cleanup sent updates (Storage scoped, no await)
    if !sent_ids.is_empty() {
        let storage = Storage::open(storage_path, storage_key)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        for id in &sent_ids {
            let _ = storage.delete_pending_update(id);
        }
    }

    Ok(MobileSyncResult {
        contacts_added,
        cards_updated,
        updates_sent,
        total: contacts_added + cards_updated + updates_sent,
        has_changes: contacts_added > 0 || cards_updated > 0 || updates_sent > 0,
        updated_contact_names,
    })
}

// === Helper Functions ===

/// Parse a hex-encoded 32-byte key.
fn parse_hex_key(hex_str: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(arr)
}

/// Update a contact's display name if it differs from the given name.
fn update_contact_name_if_needed(storage: &Storage, contact_id: &str, new_name: &str) {
    if let Ok(Some(mut contact)) = storage.load_contact(contact_id)
        && contact.display_name() != new_name
        && contact.set_display_name(new_name).is_ok()
    {
        let _ = storage.save_contact(&contact);
    }
}

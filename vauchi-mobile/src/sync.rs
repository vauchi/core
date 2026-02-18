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
use vauchi_core::sync::{
    process_card_updates as core_process_card_updates, ContactSyncData, DeviceSyncOrchestrator,
    SyncItem,
};
use vauchi_core::{Contact, ContactCard, Identity, Storage, SymmetricKey};

use crate::cert_pinning::{self, WsStream};
use crate::error::MobileError;
use crate::protocol::{
    self, create_device_sync_ack, AckStatus, DeviceSyncMessage, EncryptedUpdate, ExchangeMessage,
    MessagePayload,
};
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
    /// Device sync messages (inter-device synchronization).
    pub device_sync_messages: Vec<DeviceSyncMessage>,
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
    let mut device_sync_messages = Vec::new();

    loop {
        let msg = match tokio::time::timeout(Duration::from_secs(1), socket.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(_))) | Ok(None) => break,
            Err(_) => break, // Timeout — no more pending messages
        };

        match msg {
            Message::Binary(data) => {
                if let Ok(envelope) = protocol::decode_message(&data) {
                    match envelope.payload {
                        MessagePayload::EncryptedUpdate(update) => {
                            classify_and_store_message(
                                update,
                                &mut legacy_exchange_messages,
                                &mut encrypted_exchange_messages,
                                &mut card_updates,
                            );

                            // Send acknowledgment
                            let ack = protocol::create_ack(
                                &envelope.message_id,
                                AckStatus::ReceivedByRecipient,
                            );
                            if let Ok(ack_data) = protocol::encode_message(&ack) {
                                let _ = socket.send(Message::Binary(ack_data)).await;
                            }
                        }
                        MessagePayload::DeviceSyncMessage(msg) => {
                            let version = msg.version;
                            device_sync_messages.push(msg);

                            let ack = create_device_sync_ack(&envelope.message_id, version);
                            if let Ok(ack_data) = protocol::encode_message(&ack) {
                                let _ = socket.send(Message::Binary(ack_data)).await;
                            }
                        }
                        _ => {}
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
        device_sync_messages,
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
    if ExchangeMessage::is_exchange(&update.ciphertext) {
        if let Some(exchange) = ExchangeMessage::from_bytes(&update.ciphertext) {
            legacy_exchange.push(exchange);
            return;
        }
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

        // Record for inter-device sync
        let _ = record_contact_for_device_sync(identity, storage, &contact);

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
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

        // Record for inter-device sync
        let _ = record_contact_for_device_sync(identity, storage, &contact);

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
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
        ciphertext: encrypted_msg.to_bytes(),
    };

    let envelope = protocol::create_envelope(MessagePayload::EncryptedUpdate(update));
    let data = protocol::encode_message(&envelope).map_err(MobileError::SyncFailed)?;
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
) -> Result<u32, MobileError> {
    let result = core_process_card_updates(identity, storage, updates)?;
    Ok(result.processed)
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

/// Processes incoming device sync messages from other devices.
fn process_device_sync_messages(
    identity: &Identity,
    storage: &Storage,
    messages: Vec<DeviceSyncMessage>,
) -> Result<u32, MobileError> {
    if messages.is_empty() {
        return Ok(0);
    }

    // Try to load device registry - if none exists, skip
    let registry = match storage.load_device_registry()? {
        Some(r) if r.device_count() > 1 => r,
        _ => return Ok(0),
    };

    let mut orchestrator =
        DeviceSyncOrchestrator::new(storage, identity.create_device_info(), registry.clone());

    let mut processed = 0u32;

    for msg in messages {
        // Parse sender device ID
        let sender_device_id: [u8; 32] = match hex::decode(&msg.sender_device_id) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            _ => continue,
        };

        // Find sender in registry
        let sender_device = match registry.find_device(&sender_device_id) {
            Some(d) => d,
            None => continue,
        };

        // Decrypt payload
        let plaintext = match orchestrator
            .decrypt_from_device(&sender_device.exchange_public_key, &msg.encrypted_payload)
        {
            Ok(pt) => pt,
            Err(_) => continue,
        };

        // Parse SyncItems
        let items: Vec<SyncItem> = match serde_json::from_slice(&plaintext) {
            Ok(items) => items,
            Err(_) => continue,
        };

        // Process items with conflict resolution
        let applied = match orchestrator.process_incoming(items) {
            Ok(applied) => applied,
            Err(_) => continue,
        };

        // Apply the items
        for item in &applied {
            let _ = apply_sync_item(storage, item);
        }

        if !applied.is_empty() {
            processed += 1;
        }
    }

    Ok(processed)
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
        .map_err(MobileError::NetworkError)?;

    send_handshake(&mut socket, identity, Some(&device_id_hex)).await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let received = receive_pending(&mut socket).await?;

    // Phase 2: Process received messages (Storage scoped, no await)
    let (
        contacts_added,
        exchange_responses,
        cards_updated,
        device_synced,
        device_envelopes,
        pending_updates,
    ) = {
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

        // Process card updates
        let cards_updated = process_card_updates(identity, &storage, received.card_updates)?;

        // Process device sync messages
        let device_synced =
            process_device_sync_messages(identity, &storage, received.device_sync_messages)?;

        // Build device sync envelopes
        let device_envelopes =
            vauchi_core::sync::build_device_sync_envelopes(identity, &storage).unwrap_or_default();

        // Collect pending updates
        let pending_updates = collect_pending_updates_data(identity, &storage);

        (
            contacts_added,
            responses,
            cards_updated,
            device_synced,
            device_envelopes,
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

    // Send device sync envelopes
    let mut device_sync_sent = 0u32;
    for data in device_envelopes {
        if socket.send(Message::Binary(data)).await.is_ok() {
            device_sync_sent += 1;
        }
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
        cards_updated: cards_updated + device_synced,
        updates_sent: updates_sent + device_sync_sent,
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
    if let Ok(Some(mut contact)) = storage.load_contact(contact_id) {
        if contact.display_name() != new_name && contact.set_display_name(new_name).is_ok() {
            let _ = storage.save_contact(&contact);
        }
    }
}

/// Records a contact addition for inter-device sync.
fn record_contact_for_device_sync(
    identity: &Identity,
    storage: &Storage,
    contact: &Contact,
) -> Result<(), MobileError> {
    // Try to load device registry - if none exists or only one device, skip
    let registry = match storage.load_device_registry()? {
        Some(r) if r.device_count() > 1 => r,
        _ => return Ok(()), // No other devices to sync to
    };

    // Create orchestrator
    let mut orchestrator =
        DeviceSyncOrchestrator::new(storage, identity.create_device_info(), registry);

    // Create ContactSyncData from the contact
    let contact_data = ContactSyncData::from_contact(contact);

    // Record the sync item
    let item = SyncItem::ContactAdded {
        contact_data,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    orchestrator
        .record_local_change(item)
        .map_err(|e| MobileError::SyncFailed(format!("Failed to record device sync: {:?}", e)))?;

    Ok(())
}

/// Applies a single sync item to local storage.
fn apply_sync_item(storage: &Storage, item: &SyncItem) -> Result<(), MobileError> {
    match item {
        SyncItem::ContactAdded { contact_data, .. } => {
            if let Ok(contact) = contact_data.to_contact() {
                storage.save_contact(&contact)?;
            }
        }
        SyncItem::ContactRemoved { contact_id, .. } => {
            storage.delete_contact(contact_id)?;
        }
        SyncItem::CardUpdated {
            field_label,
            new_value,
            ..
        } => {
            if let Ok(Some(mut card)) = storage.load_own_card() {
                if card.update_field_value(field_label, new_value).is_ok() {
                    storage.save_own_card(&card)?;
                }
            }
        }
        SyncItem::VisibilityChanged {
            contact_id,
            field_label,
            is_visible,
            ..
        } => {
            if let Some(mut contact) = storage.load_contact(contact_id)? {
                if *is_visible {
                    contact.visibility_rules_mut().set_everyone(field_label);
                } else {
                    contact.visibility_rules_mut().set_nobody(field_label);
                }
                storage.save_contact(&contact)?;
            }
        }
        SyncItem::LabelChange { .. } => {
            // Label changes are handled by the label manager during full sync
        }
        SyncItem::ContactTrustChanged {
            contact_id,
            recovery_trusted,
            ..
        } => {
            if let Some(mut contact) = storage.load_contact(contact_id)? {
                contact.set_recovery_trusted(*recovery_trusted);
                storage.save_contact(&contact)?;
            }
        }
        SyncItem::DeletionScheduled {
            scheduled_at,
            execute_at,
            ..
        } => {
            let state = vauchi_core::storage::DeletionState::Scheduled {
                scheduled_at: *scheduled_at,
                execute_at: *execute_at,
            };
            storage.save_deletion_state(&state)?;
        }
        SyncItem::DeletionCancelled { .. } => {
            storage.save_deletion_state(&vauchi_core::storage::DeletionState::None)?;
        }
    }
    Ok(())
}

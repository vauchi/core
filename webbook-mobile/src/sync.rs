//! Sync operations for relay communication.
//!
//! This module handles sending and receiving messages through the relay,
//! including exchange messages and card updates.

use std::net::TcpStream;
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use webbook_core::crypto::ratchet::DoubleRatchetState;
use webbook_core::exchange::{EncryptedExchangeMessage, X3DHKeyPair};
use webbook_core::{Contact, ContactCard, Identity, Storage};

use crate::cert_pinning;
use crate::error::MobileError;
use crate::protocol::{
    self, AckStatus, EncryptedUpdate, ExchangeMessage, Handshake, MessagePayload,
};
use crate::types::MobileSyncResult;

/// Result of receiving pending messages from relay.
pub struct ReceivedMessages {
    /// Legacy plaintext exchange messages (backward compatibility).
    pub legacy_exchange: Vec<ExchangeMessage>,
    /// Encrypted exchange messages (new format).
    pub encrypted_exchange: Vec<Vec<u8>>,
    /// Card updates from existing contacts: (sender_id, ciphertext).
    pub card_updates: Vec<(String, Vec<u8>)>,
}

/// Sends handshake to relay.
pub fn send_handshake(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    client_id: &str,
) -> Result<(), MobileError> {
    let handshake = Handshake {
        client_id: client_id.to_string(),
    };
    let envelope = protocol::create_envelope(MessagePayload::Handshake(handshake));
    let data = protocol::encode_message(&envelope)
        .map_err(|e| MobileError::SyncFailed(format!("Encode error: {}", e)))?;
    socket
        .send(Message::Binary(data))
        .map_err(|e| MobileError::NetworkError(e.to_string()))?;
    Ok(())
}

/// Receives pending messages from relay.
///
/// Classifies incoming messages into:
/// - Legacy plaintext exchange messages
/// - Encrypted exchange messages
/// - Card updates (ratchet-encrypted)
#[allow(clippy::type_complexity)]
pub fn receive_pending(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<ReceivedMessages, MobileError> {
    let mut legacy_exchange_messages = Vec::new();
    let mut encrypted_exchange_messages = Vec::new();
    let mut card_updates = Vec::new();

    loop {
        match socket.read() {
            Ok(Message::Binary(data)) => {
                if let Ok(envelope) = protocol::decode_message(&data) {
                    if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                        classify_and_store_message(
                            update,
                            &mut legacy_exchange_messages,
                            &mut encrypted_exchange_messages,
                            &mut card_updates,
                        );

                        // Send acknowledgment
                        send_ack(socket, &envelope.message_id);
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = socket.send(Message::Pong(data));
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => { /* Ignore other message types */ }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No more messages
                break;
            }
            Err(_) => break,
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

/// Sends an acknowledgment for a received message.
fn send_ack(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, message_id: &str) {
    let ack = protocol::create_ack(message_id, AckStatus::ReceivedByRecipient);
    if let Ok(ack_data) = protocol::encode_message(&ack) {
        let _ = socket.send(Message::Binary(ack_data));
    }
}

/// Processes legacy plaintext exchange messages and creates new contacts.
pub fn process_legacy_exchange_messages(
    identity: &Identity,
    storage: &Storage,
    messages: Vec<ExchangeMessage>,
    relay_url: &str,
    pinned_cert: Option<&str>,
) -> Result<u32, MobileError> {
    let mut added = 0u32;
    let our_x3dh = identity.x3dh_keypair();

    for exchange in messages {
        // Parse identity key
        let identity_key = match parse_hex_key(&exchange.identity_public_key) {
            Some(key) => key,
            None => continue,
        };

        let public_id = hex::encode(identity_key);

        // Handle response to our exchange (update contact name)
        if exchange.is_response {
            update_contact_name_if_needed(storage, &public_id, &exchange.display_name);
            continue;
        }

        // Check if contact already exists
        if storage.load_contact(&public_id)?.is_some() {
            continue;
        }

        // Parse ephemeral key
        let ephemeral_key = match parse_hex_key(&exchange.ephemeral_public_key) {
            Some(key) => key,
            None => continue,
        };

        // Perform X3DH as responder
        let shared_secret =
            match webbook_core::exchange::X3DH::respond(&our_x3dh, &identity_key, &ephemeral_key) {
                Ok(secret) => secret,
                Err(_) => continue,
            };

        // Create and save contact
        let card = ContactCard::new(&exchange.display_name);
        let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact)?;

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        let _ = storage.save_ratchet_state(&contact_id, &ratchet, true);

        added += 1;

        // Send encrypted exchange response
        let _ = send_exchange_response(identity, &public_id, &ephemeral_key, relay_url, pinned_cert);
    }

    Ok(added)
}

/// Processes encrypted exchange messages (new format with proper encryption).
pub fn process_encrypted_exchange_messages(
    identity: &Identity,
    storage: &Storage,
    encrypted_data: Vec<Vec<u8>>,
    relay_url: &str,
    pinned_cert: Option<&str>,
) -> Result<u32, MobileError> {
    let mut added = 0u32;
    let our_x3dh = identity.x3dh_keypair();

    for data in encrypted_data {
        // Try to parse as EncryptedExchangeMessage
        let encrypted_msg = match EncryptedExchangeMessage::from_bytes(&data) {
            Ok(msg) => msg,
            Err(_) => continue,
        };

        // Decrypt to get sender's info
        let (payload, shared_secret) = match encrypted_msg.decrypt(&our_x3dh) {
            Ok(result) => result,
            Err(_) => continue,
        };

        let public_id = hex::encode(payload.identity_key);

        // Check if contact already exists
        if storage.load_contact(&public_id)?.is_some() {
            // Contact exists - might be a response, update name if needed
            update_contact_name_if_needed(storage, &public_id, &payload.display_name);
            continue;
        }

        // Create new contact
        let card = ContactCard::new(&payload.display_name);
        let contact = Contact::from_exchange(payload.identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact)?;

        // Initialize ratchet as responder
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        let _ = storage.save_ratchet_state(&contact_id, &ratchet, false);

        added += 1;

        // Send encrypted exchange response
        let _ = send_exchange_response(
            identity,
            &public_id,
            &payload.exchange_key,
            relay_url,
            pinned_cert,
        );
    }

    Ok(added)
}

/// Sends encrypted exchange response with our identity and name.
pub fn send_exchange_response(
    identity: &Identity,
    recipient_id: &str,
    recipient_exchange_key: &[u8; 32],
    relay_url: &str,
    pinned_cert: Option<&str>,
) -> Result<(), MobileError> {
    let mut socket =
        cert_pinning::connect_with_pinning(relay_url, pinned_cert).map_err(MobileError::NetworkError)?;

    let our_id = identity.public_id();
    send_handshake(&mut socket, &our_id)?;

    // Create encrypted exchange message using X3DH
    let our_x3dh = identity.x3dh_keypair();
    let (encrypted_msg, _shared_secret) = EncryptedExchangeMessage::create(
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
        .map_err(|e| MobileError::NetworkError(e.to_string()))?;

    std::thread::sleep(Duration::from_millis(100));
    let _ = socket.close(None);

    Ok(())
}

/// Processes incoming card updates from existing contacts.
pub fn process_card_updates(
    storage: &Storage,
    updates: Vec<(String, Vec<u8>)>,
) -> Result<u32, MobileError> {
    let mut processed = 0u32;

    for (sender_id, ciphertext) in updates {
        // Get contact
        let mut contact = match storage.load_contact(&sender_id)? {
            Some(c) => c,
            None => continue,
        };

        // Get ratchet state
        let (mut ratchet, _is_initiator) = match storage.load_ratchet_state(&sender_id)? {
            Some(state) => state,
            None => continue,
        };

        // Try to parse as a RatchetMessage from JSON
        let ratchet_msg: webbook_core::crypto::ratchet::RatchetMessage =
            match serde_json::from_slice(&ciphertext) {
                Ok(msg) => msg,
                Err(_) => continue,
            };

        // Decrypt the card delta
        let plaintext = match ratchet.decrypt(&ratchet_msg) {
            Ok(pt) => pt,
            Err(_) => continue,
        };

        // Parse and apply delta
        if let Ok(delta) = serde_json::from_slice::<webbook_core::sync::CardDelta>(&plaintext) {
            let mut card = contact.card().clone();
            if delta.apply(&mut card).is_ok() {
                contact.update_card(card);
                storage.save_contact(&contact)?;
                processed += 1;
            }
        }

        // Save updated ratchet state
        let _ = storage.save_ratchet_state(&sender_id, &ratchet, false);
    }

    Ok(processed)
}

/// Sends pending outbound updates to contacts.
pub fn send_pending_updates(
    identity: &Identity,
    storage: &Storage,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<u32, MobileError> {
    let contacts = storage.list_contacts()?;
    let our_id = identity.public_id();
    let mut sent = 0u32;

    for contact in contacts {
        let pending = storage.get_pending_updates(contact.id())?;

        for update in pending {
            let msg = EncryptedUpdate {
                recipient_id: contact.id().to_string(),
                sender_id: our_id.clone(),
                ciphertext: update.payload,
            };

            let envelope = protocol::create_envelope(MessagePayload::EncryptedUpdate(msg));
            if let Ok(data) = protocol::encode_message(&envelope) {
                if socket.send(Message::Binary(data)).is_ok() {
                    let _ = storage.delete_pending_update(&update.id);
                    sent += 1;
                }
            }
        }
    }

    Ok(sent)
}

/// Performs a complete sync operation.
pub fn do_sync(
    identity: &Identity,
    storage: &Storage,
    relay_url: &str,
    pinned_cert: Option<&str>,
) -> Result<MobileSyncResult, MobileError> {
    let client_id = identity.public_id();

    // Connect to relay
    let mut socket =
        cert_pinning::connect_with_pinning(relay_url, pinned_cert).map_err(MobileError::NetworkError)?;

    // Set read timeout for non-blocking receive
    if let MaybeTlsStream::Plain(ref stream) = socket.get_ref() {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
    }

    // Send handshake
    send_handshake(&mut socket, &client_id)?;

    // Wait briefly for server to send pending messages
    std::thread::sleep(Duration::from_millis(500));

    // Receive and classify pending messages
    let received = receive_pending(&mut socket)?;

    // Process legacy plaintext exchange messages
    let legacy_added = process_legacy_exchange_messages(
        identity,
        storage,
        received.legacy_exchange,
        relay_url,
        pinned_cert,
    )?;

    // Process encrypted exchange messages
    let encrypted_added = process_encrypted_exchange_messages(
        identity,
        storage,
        received.encrypted_exchange,
        relay_url,
        pinned_cert,
    )?;

    let contacts_added = legacy_added + encrypted_added;

    // Process card updates
    let cards_updated = process_card_updates(storage, received.card_updates)?;

    // Send pending outbound updates
    let updates_sent = send_pending_updates(identity, storage, &mut socket)?;

    // Close connection
    let _ = socket.close(None);

    Ok(MobileSyncResult {
        contacts_added,
        cards_updated,
        updates_sent,
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

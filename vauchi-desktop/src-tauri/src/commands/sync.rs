//! Sync Commands
//!
//! Handles synchronization with the relay server.

use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::State;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::{EncryptedExchangeMessage, X3DHKeyPair};
use vauchi_core::network::simple_message::{
    create_simple_ack, create_simple_envelope, decode_simple_message, encode_simple_message,
    LegacyExchangeMessage, SimpleAckStatus, SimpleEncryptedUpdate, SimpleHandshake, SimplePayload,
};
use vauchi_core::{Contact, ContactCard, Identity, Storage};

use crate::state::AppState;

/// Result of a sync operation.
#[derive(Serialize)]
pub struct SyncResult {
    /// Number of contacts added from exchange messages.
    pub contacts_added: u32,
    /// Number of contact cards updated.
    pub cards_updated: u32,
    /// Number of outbound updates sent.
    pub updates_sent: u32,
    /// Whether sync completed successfully.
    pub success: bool,
    /// Error message if sync failed.
    pub error: Option<String>,
}

/// Sync status for display.
#[derive(Serialize)]
pub struct SyncStatus {
    /// Number of pending outbound updates.
    pub pending_updates: u32,
    /// Last sync timestamp (Unix seconds), if available.
    pub last_sync: Option<u64>,
    /// Whether currently syncing.
    pub is_syncing: bool,
}

/// Connect to relay server via WebSocket.
fn connect_to_relay(relay_url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let (socket, _response) = tungstenite::connect(relay_url)
        .map_err(|e| format!("Failed to connect to relay: {}", e))?;
    Ok(socket)
}

/// Send handshake to relay.
fn send_handshake(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    client_id: &str,
) -> Result<(), String> {
    let handshake = SimpleHandshake {
        client_id: client_id.to_string(),
    };
    let envelope = create_simple_envelope(SimplePayload::Handshake(handshake));
    let data = encode_simple_message(&envelope).map_err(|e| format!("Encode error: {}", e))?;
    socket
        .send(Message::Binary(data))
        .map_err(|e| format!("Send error: {}", e))?;
    Ok(())
}

/// Received messages from relay.
struct ReceivedMessages {
    legacy_exchange: Vec<LegacyExchangeMessage>,
    encrypted_exchange: Vec<Vec<u8>>,
    card_updates: Vec<(String, Vec<u8>)>,
}

/// Receive pending messages from relay.
fn receive_pending(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<ReceivedMessages, String> {
    let mut legacy_exchange = Vec::new();
    let mut encrypted_exchange = Vec::new();
    let mut card_updates = Vec::new();

    // Set read timeout for non-blocking receive
    if let MaybeTlsStream::Plain(ref stream) = socket.get_ref() {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
    }

    loop {
        match socket.read() {
            Ok(Message::Binary(data)) => {
                if let Ok(envelope) = decode_simple_message(&data) {
                    if let SimplePayload::EncryptedUpdate(update) = envelope.payload {
                        // Classify the message
                        if LegacyExchangeMessage::is_exchange(&update.ciphertext) {
                            if let Some(exchange) =
                                LegacyExchangeMessage::from_bytes(&update.ciphertext)
                            {
                                legacy_exchange.push(exchange);
                            }
                        } else if EncryptedExchangeMessage::from_bytes(&update.ciphertext).is_ok() {
                            encrypted_exchange.push(update.ciphertext);
                        } else {
                            card_updates.push((update.sender_id, update.ciphertext));
                        }

                        // Send acknowledgment
                        let ack =
                            create_simple_ack(&envelope.message_id, SimpleAckStatus::ReceivedByRecipient);
                        if let Ok(ack_data) = encode_simple_message(&ack) {
                            let _ = socket.send(Message::Binary(ack_data));
                        }
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
                break;
            }
            Err(_) => break,
        }
    }

    Ok(ReceivedMessages {
        legacy_exchange,
        encrypted_exchange,
        card_updates,
    })
}

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

/// Process legacy plaintext exchange messages.
fn process_legacy_exchanges(
    identity: &Identity,
    storage: &Storage,
    messages: Vec<LegacyExchangeMessage>,
    relay_url: &str,
) -> Result<u32, String> {
    let mut added = 0u32;
    let our_x3dh = identity.x3dh_keypair();

    for exchange in messages {
        let identity_key = match parse_hex_key(&exchange.identity_public_key) {
            Some(key) => key,
            None => continue,
        };

        let public_id = hex::encode(identity_key);

        // Handle response (update contact name)
        if exchange.is_response {
            if let Ok(Some(mut contact)) = storage.load_contact(&public_id) {
                if contact.display_name() != exchange.display_name {
                    if contact.set_display_name(&exchange.display_name).is_ok() {
                        let _ = storage.save_contact(&contact);
                    }
                }
            }
            continue;
        }

        // Check if contact exists
        if storage
            .load_contact(&public_id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            continue;
        }

        let ephemeral_key = match parse_hex_key(&exchange.ephemeral_public_key) {
            Some(key) => key,
            None => continue,
        };

        // Perform X3DH
        let shared_secret =
            match vauchi_core::exchange::X3DH::respond(&our_x3dh, &identity_key, &ephemeral_key) {
                Ok(secret) => secret,
                Err(_) => continue,
            };

        // Create contact
        let card = ContactCard::new(&exchange.display_name);
        let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact).map_err(|e| e.to_string())?;

        // Initialize ratchet
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        let _ = storage.save_ratchet_state(&contact_id, &ratchet, true);

        added += 1;

        // Send response
        let _ = send_exchange_response(identity, &public_id, &ephemeral_key, relay_url);
    }

    Ok(added)
}

/// Process encrypted exchange messages.
fn process_encrypted_exchanges(
    identity: &Identity,
    storage: &Storage,
    encrypted_data: Vec<Vec<u8>>,
    relay_url: &str,
) -> Result<u32, String> {
    let mut added = 0u32;
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

        // Check if contact exists
        if storage
            .load_contact(&public_id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            continue;
        }

        // Create contact
        let card = ContactCard::new(&payload.display_name);
        let contact = Contact::from_exchange(payload.identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        storage.save_contact(&contact).map_err(|e| e.to_string())?;

        // Initialize ratchet
        let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
        let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
        let _ = storage.save_ratchet_state(&contact_id, &ratchet, false);

        added += 1;

        // Send response
        let _ = send_exchange_response(identity, &public_id, &payload.exchange_key, relay_url);
    }

    Ok(added)
}

/// Send exchange response.
fn send_exchange_response(
    identity: &Identity,
    recipient_id: &str,
    recipient_exchange_key: &[u8; 32],
    relay_url: &str,
) -> Result<(), String> {
    let mut socket = connect_to_relay(relay_url)?;

    let our_id = identity.public_id();
    send_handshake(&mut socket, &our_id)?;

    let our_x3dh = identity.x3dh_keypair();
    let (encrypted_msg, _) = EncryptedExchangeMessage::create(
        &our_x3dh,
        recipient_exchange_key,
        identity.signing_public_key(),
        identity.display_name(),
    )
    .map_err(|e| format!("Failed to encrypt exchange: {:?}", e))?;

    let update = SimpleEncryptedUpdate {
        recipient_id: recipient_id.to_string(),
        sender_id: our_id,
        ciphertext: encrypted_msg.to_bytes(),
    };

    let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(update));
    let data = encode_simple_message(&envelope).map_err(|e| e.to_string())?;
    socket
        .send(Message::Binary(data))
        .map_err(|e| e.to_string())?;

    std::thread::sleep(Duration::from_millis(100));
    let _ = socket.close(None);

    Ok(())
}

/// Process incoming card updates.
fn process_card_updates(storage: &Storage, updates: Vec<(String, Vec<u8>)>) -> Result<u32, String> {
    let mut processed = 0u32;

    for (sender_id, ciphertext) in updates {
        let mut contact = match storage.load_contact(&sender_id).map_err(|e| e.to_string())? {
            Some(c) => c,
            None => continue,
        };

        let (mut ratchet, _) = match storage
            .load_ratchet_state(&sender_id)
            .map_err(|e| e.to_string())?
        {
            Some(state) => state,
            None => continue,
        };

        let ratchet_msg: vauchi_core::crypto::ratchet::RatchetMessage =
            match serde_json::from_slice(&ciphertext) {
                Ok(msg) => msg,
                Err(_) => continue,
            };

        let plaintext = match ratchet.decrypt(&ratchet_msg) {
            Ok(pt) => pt,
            Err(_) => continue,
        };

        if let Ok(delta) = serde_json::from_slice::<vauchi_core::sync::CardDelta>(&plaintext) {
            let mut card = contact.card().clone();
            if delta.apply(&mut card).is_ok() {
                contact.update_card(card);
                storage.save_contact(&contact).map_err(|e| e.to_string())?;
                processed += 1;
            }
        }

        let _ = storage.save_ratchet_state(&sender_id, &ratchet, false);
    }

    Ok(processed)
}

/// Send pending outbound updates.
fn send_pending_updates(
    identity: &Identity,
    storage: &Storage,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<u32, String> {
    let contacts = storage.list_contacts().map_err(|e| e.to_string())?;
    let our_id = identity.public_id();
    let mut sent = 0u32;

    for contact in contacts {
        let pending = storage
            .get_pending_updates(contact.id())
            .map_err(|e| e.to_string())?;

        for update in pending {
            let msg = SimpleEncryptedUpdate {
                recipient_id: contact.id().to_string(),
                sender_id: our_id.clone(),
                ciphertext: update.payload,
            };

            let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(msg));
            if let Ok(data) = encode_simple_message(&envelope) {
                if socket.send(Message::Binary(data)).is_ok() {
                    let _ = storage.delete_pending_update(&update.id);
                    sent += 1;
                }
            }
        }
    }

    Ok(sent)
}

/// Perform a sync with the relay server.
///
/// This sends pending updates to contacts and receives incoming updates.
#[tauri::command]
pub fn sync(state: State<'_, Mutex<AppState>>) -> Result<SyncResult, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or("No identity found. Please create an identity first.")?;

    let relay_url = state.relay_url();
    let client_id = identity.public_id();

    // Connect to relay
    let mut socket = connect_to_relay(relay_url)?;

    // Send handshake
    send_handshake(&mut socket, &client_id)?;

    // Wait briefly for server to send pending messages
    std::thread::sleep(Duration::from_millis(500));

    // Receive pending messages
    let received = receive_pending(&mut socket)?;

    // Process legacy exchange messages
    let legacy_added =
        process_legacy_exchanges(identity, &state.storage, received.legacy_exchange, relay_url)?;

    // Process encrypted exchange messages
    let encrypted_added = process_encrypted_exchanges(
        identity,
        &state.storage,
        received.encrypted_exchange,
        relay_url,
    )?;

    let contacts_added = legacy_added + encrypted_added;

    // Process card updates
    let cards_updated = process_card_updates(&state.storage, received.card_updates)?;

    // Send pending outbound updates
    let updates_sent = send_pending_updates(identity, &state.storage, &mut socket)?;

    // Close connection
    let _ = socket.close(None);

    Ok(SyncResult {
        contacts_added,
        cards_updated,
        updates_sent,
        success: true,
        error: None,
    })
}

/// Get the current sync status.
#[tauri::command]
pub fn get_sync_status(state: State<'_, Mutex<AppState>>) -> Result<SyncStatus, String> {
    let state = state.lock().unwrap();

    if state.identity.is_none() {
        return Ok(SyncStatus {
            pending_updates: 0,
            last_sync: None,
            is_syncing: false,
        });
    }

    // Count pending updates across all contacts
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let mut total_pending = 0u32;
    for contact in &contacts {
        let pending = state
            .storage
            .get_pending_updates(contact.id())
            .unwrap_or_default();
        total_pending += pending.len() as u32;
    }

    Ok(SyncStatus {
        pending_updates: total_pending,
        last_sync: None,
        is_syncing: false,
    })
}

/// Get the current relay URL.
#[tauri::command]
pub fn get_relay_url(state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    let state = state.lock().unwrap();
    Ok(state.relay_url().to_string())
}

/// Set the relay URL.
#[tauri::command]
pub fn set_relay_url(state: State<'_, Mutex<AppState>>, url: String) -> Result<(), String> {
    let mut state = state.lock().unwrap();
    state.set_relay_url(&url).map_err(|e| e.to_string())
}

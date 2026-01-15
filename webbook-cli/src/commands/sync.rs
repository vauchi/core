//! Sync Command
//!
//! Synchronize with the relay server.

use std::fs;
use std::net::TcpStream;

use anyhow::{bail, Result};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use webbook_core::{WebBook, WebBookConfig, Identity, IdentityBackup, Contact};
use webbook_core::network::MockTransport;
use webbook_core::exchange::X3DH;

use crate::config::CliConfig;
use crate::display;
use crate::protocol::{
    MessagePayload, Handshake, AckStatus, EncryptedUpdate,
    ExchangeMessage, create_envelope, encode_message, decode_message, create_ack,
};

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "webbook-local-storage";

/// Opens WebBook from the config and loads the identity.
fn open_webbook(config: &CliConfig) -> Result<WebBook<MockTransport>> {
    if !config.is_initialized() {
        bail!("WebBook not initialized. Run 'webbook init <name>' first.");
    }

    let wb_config = WebBookConfig::with_storage_path(&config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key());

    let mut wb = WebBook::new(wb_config)?;

    // Load identity from file
    let backup_data = fs::read(config.identity_path())?;
    let backup = IdentityBackup::new(backup_data);
    let identity = Identity::import_backup(&backup, LOCAL_STORAGE_PASSWORD)?;
    wb.set_identity(identity)?;

    Ok(wb)
}

/// Sends handshake message to relay.
fn send_handshake(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    client_id: &str,
) -> Result<()> {
    let handshake = Handshake {
        client_id: client_id.to_string(),
    };
    let envelope = create_envelope(MessagePayload::Handshake(handshake));
    let data = encode_message(&envelope).map_err(|e| anyhow::anyhow!(e))?;
    socket.send(Message::Binary(data))?;
    Ok(())
}

/// Sends an exchange response with our name to a contact.
fn send_exchange_response(
    config: &CliConfig,
    our_identity: &Identity,
    recipient_id: &str,
) -> Result<()> {
    // Connect to relay
    let (mut socket, _) = connect(&config.relay_url)?;

    // Send handshake
    let our_id = our_identity.public_id();
    send_handshake(&mut socket, &our_id)?;

    // Get our exchange key for the message
    let exchange_key_slice = our_identity.exchange_public_key();
    let exchange_key: [u8; 32] = exchange_key_slice
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid exchange key length"))?;

    // Create response message
    let exchange_msg = ExchangeMessage::new_response(
        our_identity.signing_public_key(),
        &exchange_key,
        our_identity.display_name(),
    );

    // Create encrypted update
    let update = EncryptedUpdate {
        recipient_id: recipient_id.to_string(),
        sender_id: our_id,
        ciphertext: exchange_msg.to_bytes(),
    };

    let envelope = create_envelope(MessagePayload::EncryptedUpdate(update));
    let data = encode_message(&envelope).map_err(|e| anyhow::anyhow!(e))?;
    socket.send(Message::Binary(data))?;

    // Wait briefly for acknowledgment
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Close connection
    let _ = socket.close(None);

    Ok(())
}

/// Receives and processes pending messages from relay.
/// Returns: (total_received, exchange_messages, encrypted_card_updates)
fn receive_pending(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    _wb: &WebBook<MockTransport>,
) -> Result<(usize, Vec<ExchangeMessage>, Vec<(String, Vec<u8>)>)> {
    let mut received = 0;
    let mut exchange_messages = Vec::new();
    let mut card_updates = Vec::new();  // (sender_id, ciphertext)

    // Set a read timeout so we don't block forever
    // The relay sends pending messages immediately after handshake
    loop {
        match socket.read() {
            Ok(Message::Binary(data)) => {
                match decode_message(&data) {
                    Ok(envelope) => {
                        match envelope.payload {
                            MessagePayload::EncryptedUpdate(update) => {
                                received += 1;

                                // Check if this is an exchange message
                                if ExchangeMessage::is_exchange(&update.ciphertext) {
                                    if let Some(exchange) = ExchangeMessage::from_bytes(&update.ciphertext) {
                                        display::info(&format!(
                                            "Received exchange request from {}",
                                            exchange.display_name
                                        ));
                                        exchange_messages.push(exchange);
                                    }
                                } else {
                                    // This is an encrypted card update
                                    display::info(&format!(
                                        "Received encrypted update from {}",
                                        &update.sender_id[..8]
                                    ));
                                    card_updates.push((update.sender_id.clone(), update.ciphertext));
                                }

                                // Send acknowledgment
                                let ack = create_ack(&envelope.message_id, AckStatus::ReceivedByRecipient);
                                if let Ok(ack_data) = encode_message(&ack) {
                                    let _ = socket.send(Message::Binary(ack_data));
                                }
                            }
                            MessagePayload::Acknowledgment(ack) => {
                                display::info(&format!("Message {} acknowledged", &ack.message_id[..8]));
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        display::warning(&format!("Failed to decode message: {}", e));
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = socket.send(Message::Pong(data));
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Ok(_) => {
                // Ignore text messages, pongs, etc.
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No more messages available
                break;
            }
            Err(e) => {
                // Connection error or closed
                display::warning(&format!("Connection issue: {}", e));
                break;
            }
        }
    }

    Ok((received, exchange_messages, card_updates))
}

/// Processes exchange messages and creates contacts.
fn process_exchange_messages(
    wb: &WebBook<MockTransport>,
    messages: Vec<ExchangeMessage>,
    config: &CliConfig,
) -> Result<(usize, usize)> {
    let mut added = 0;
    let mut updated = 0;

    // Get our identity for X3DH
    let identity = wb.identity().ok_or_else(|| anyhow::anyhow!("No identity found"))?;
    let our_x3dh = identity.x3dh_keypair();

    for exchange in messages {
        // Parse the identity public key (signing key)
        let identity_key = match hex::decode(&exchange.identity_public_key) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            _ => {
                display::warning(&format!(
                    "Invalid identity key from {}",
                    exchange.display_name
                ));
                continue;
            }
        };

        let public_id = hex::encode(&identity_key);

        // Check if this is a response to our exchange
        if exchange.is_response {
            // Update existing contact's name
            if let Some(mut contact) = wb.get_contact(&public_id)? {
                if contact.display_name() != exchange.display_name {
                    if let Err(e) = contact.set_display_name(&exchange.display_name) {
                        display::warning(&format!(
                            "Failed to update contact name: {:?}", e
                        ));
                        continue;
                    }
                    wb.update_contact(&contact)?;
                    display::success(&format!("Updated contact name: {}", exchange.display_name));
                    updated += 1;
                } else {
                    display::info(&format!(
                        "Contact {} already has correct name",
                        exchange.display_name
                    ));
                }
            } else {
                display::warning(&format!(
                    "Received response from unknown contact: {}",
                    exchange.display_name
                ));
            }
            continue;
        }

        // Parse the ephemeral public key (for X3DH)
        let ephemeral_key = match hex::decode(&exchange.ephemeral_public_key) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            _ => {
                display::warning(&format!(
                    "Invalid ephemeral key from {}",
                    exchange.display_name
                ));
                continue;
            }
        };

        // Check if we already have this contact
        if wb.get_contact(&public_id)?.is_some() {
            display::info(&format!("{} is already a contact", exchange.display_name));
            continue;
        }

        // Perform X3DH as responder to derive shared secret
        let shared_secret = match X3DH::respond(&our_x3dh, &identity_key, &ephemeral_key) {
            Ok(secret) => secret,
            Err(e) => {
                display::warning(&format!(
                    "X3DH key agreement failed for {}: {:?}",
                    exchange.display_name, e
                ));
                continue;
            }
        };

        // Create contact card
        let card = webbook_core::ContactCard::new(&exchange.display_name);

        // Create contact
        let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();
        wb.add_contact(contact)?;

        // Initialize Double Ratchet as responder for forward secrecy
        // Recreate the X3DH keypair since we can't clone it
        let ratchet_dh = webbook_core::exchange::X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
        if let Err(e) = wb.create_ratchet_as_responder(&contact_id, &shared_secret, ratchet_dh) {
            display::warning(&format!("Failed to initialize ratchet: {:?}", e));
        }

        display::success(&format!("Added contact: {}", exchange.display_name));
        added += 1;

        // Send our name back to them
        display::info(&format!("Sending our name to {}...", exchange.display_name));
        match send_exchange_response(config, identity, &public_id) {
            Ok(()) => {
                display::success("Response sent");
            }
            Err(e) => {
                display::warning(&format!("Could not send response: {}", e));
            }
        }
    }

    Ok((added, updated))
}

/// Processes encrypted card updates from contacts.
fn process_card_updates(
    wb: &WebBook<MockTransport>,
    updates: Vec<(String, Vec<u8>)>,  // (sender_id, ciphertext)
) -> Result<usize> {
    let mut processed = 0;

    for (sender_id, ciphertext) in updates {
        // Get contact to display name
        let contact_name = match wb.get_contact(&sender_id)? {
            Some(c) => c.display_name().to_string(),
            None => {
                display::warning(&format!("Update from unknown contact: {}...", &sender_id[..8]));
                continue;
            }
        };

        // Process the encrypted update
        match wb.process_card_update(&sender_id, &ciphertext) {
            Ok(changed_fields) => {
                if changed_fields.is_empty() {
                    display::info(&format!("{} sent an update (no changes)", contact_name));
                } else {
                    display::success(&format!(
                        "{} updated: {}",
                        contact_name,
                        changed_fields.join(", ")
                    ));
                }
                processed += 1;
            }
            Err(e) => {
                display::warning(&format!(
                    "Failed to process update from {}: {:?}",
                    contact_name, e
                ));
            }
        }
    }

    Ok(processed)
}

/// Sends pending card updates to contacts via relay.
fn send_pending_updates(
    wb: &WebBook<MockTransport>,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    our_id: &str,
) -> Result<usize> {
    // Get all contacts and check for pending updates
    let contacts = wb.list_contacts()?;
    let mut sent = 0;

    for contact in contacts {
        let pending = wb.storage().get_pending_updates(contact.id())?;

        for update in pending {
            if update.update_type != "card_delta" {
                continue;
            }

            // Create encrypted update message
            let msg = EncryptedUpdate {
                recipient_id: contact.id().to_string(),
                sender_id: our_id.to_string(),
                ciphertext: update.payload,
            };

            let envelope = create_envelope(MessagePayload::EncryptedUpdate(msg));
            match encode_message(&envelope) {
                Ok(data) => {
                    if socket.send(Message::Binary(data)).is_ok() {
                        // Mark as sent (delete from pending)
                        let _ = wb.storage().delete_pending_update(&update.id);
                        sent += 1;
                        display::info(&format!("Sent update to {}", contact.display_name()));
                    }
                }
                Err(e) => {
                    display::warning(&format!("Failed to encode update: {}", e));
                }
            }
        }
    }

    Ok(sent)
}

/// Runs the sync command.
pub async fn run(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity().ok_or_else(|| anyhow::anyhow!("No identity found"))?;
    let client_id = identity.public_id();

    println!("Connecting to {}...", config.relay_url);

    // Connect via WebSocket
    let (mut socket, response) = connect(&config.relay_url)?;

    if response.status().is_success() || response.status().as_u16() == 101 {
        display::success("Connected");
    }

    // Set read timeout on underlying socket for non-blocking receive
    if let MaybeTlsStream::Plain(ref stream) = socket.get_ref() {
        stream.set_read_timeout(Some(std::time::Duration::from_millis(1000)))?;
    }

    // Send handshake
    send_handshake(&mut socket, &client_id)?;

    // Small delay to let server send pending messages
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Receive pending messages
    let (received, exchange_messages, card_updates) = receive_pending(&mut socket, &wb)?;

    // Process exchange messages
    let (contacts_added, contacts_updated) = process_exchange_messages(&wb, exchange_messages, config)?;

    // Process encrypted card updates
    let cards_updated = process_card_updates(&wb, card_updates)?;

    // Send pending outbound updates
    let updates_sent = send_pending_updates(&wb, &mut socket, &client_id)?;

    // Close connection
    let _ = socket.close(None);

    // Display results
    println!();
    let total_changes = received + contacts_added + contacts_updated + cards_updated + updates_sent;
    if total_changes > 0 {
        let mut summary = format!("Sync complete: {} received", received);
        if contacts_added > 0 { summary.push_str(&format!(", {} contacts added", contacts_added)); }
        if contacts_updated > 0 { summary.push_str(&format!(", {} contacts updated", contacts_updated)); }
        if cards_updated > 0 { summary.push_str(&format!(", {} cards updated", cards_updated)); }
        if updates_sent > 0 { summary.push_str(&format!(", {} sent", updates_sent)); }
        display::success(&summary);
    } else {
        display::info("Sync complete: No new messages or pending updates");
    }

    Ok(())
}

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
fn receive_pending(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    _wb: &WebBook<MockTransport>,
) -> Result<(usize, Vec<ExchangeMessage>)> {
    let mut received = 0;
    let mut exchange_messages = Vec::new();

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
                                    display::info(&format!(
                                        "Received update from {}",
                                        &update.sender_id[..8]
                                    ));
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

    Ok((received, exchange_messages))
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
        let contact = Contact::from_exchange(identity_key, card, shared_secret);
        wb.add_contact(contact)?;

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
    let (received, exchange_messages) = receive_pending(&mut socket, &wb)?;

    // Process exchange messages
    let (contacts_added, contacts_updated) = process_exchange_messages(&wb, exchange_messages, config)?;

    // Close connection
    let _ = socket.close(None);

    // Display results
    println!();
    if received > 0 || contacts_added > 0 || contacts_updated > 0 {
        display::success(&format!(
            "Sync complete: {} messages received, {} contacts added, {} contacts updated",
            received, contacts_added, contacts_updated
        ));
    } else {
        display::info("Sync complete: No new messages");
    }

    Ok(())
}

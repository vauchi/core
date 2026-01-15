//! Sync Command
//!
//! Synchronize with the relay server.

use std::fs;
use std::net::TcpStream;

use anyhow::{bail, Result};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use webbook_core::{WebBook, WebBookConfig, Identity, IdentityBackup, Contact, SymmetricKey};
use webbook_core::network::MockTransport;

use crate::config::CliConfig;
use crate::display;
use crate::protocol::{
    MessagePayload, Handshake, AckStatus,
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
) -> Result<usize> {
    let mut added = 0;

    for exchange in messages {
        // Parse the identity public key
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

        // Check if we already have this contact
        let public_id = hex::encode(&identity_key);
        if wb.get_contact(&public_id)?.is_some() {
            display::info(&format!("{} is already a contact", exchange.display_name));
            continue;
        }

        // Create a shared secret (in a real implementation, use X3DH)
        // For now, derive from the ephemeral key
        let shared_secret = SymmetricKey::generate();

        // Create contact card
        let card = webbook_core::ContactCard::new(&exchange.display_name);

        // Create contact
        let contact = Contact::from_exchange(identity_key, card, shared_secret);
        wb.add_contact(contact)?;

        display::success(&format!("Added contact: {}", exchange.display_name));
        added += 1;
    }

    Ok(added)
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
    let contacts_added = process_exchange_messages(&wb, exchange_messages)?;

    // Close connection
    let _ = socket.close(None);

    // Display results
    println!();
    if received > 0 || contacts_added > 0 {
        display::success(&format!(
            "Sync complete: {} messages received, {} contacts added",
            received, contacts_added
        ));
    } else {
        display::info("Sync complete: No new messages");
    }

    Ok(())
}

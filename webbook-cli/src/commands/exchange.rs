//! Exchange Command
//!
//! Generate and complete contact exchanges.

use std::fs;
use std::net::TcpStream;

use anyhow::{bail, Result};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;
use webbook_core::{WebBook, WebBookConfig, Contact, SymmetricKey, Identity, IdentityBackup};
use webbook_core::network::MockTransport;
use webbook_core::exchange::ExchangeQR;

use crate::config::CliConfig;
use crate::display;
use crate::protocol::{
    MessagePayload, Handshake, EncryptedUpdate, ExchangeMessage,
    create_envelope, encode_message,
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

/// Sends an exchange message to a recipient via the relay.
fn send_exchange_message(
    config: &CliConfig,
    our_identity: &Identity,
    recipient_id: &str,
) -> Result<()> {
    // Connect to relay
    let (mut socket, _) = connect(&config.relay_url)?;

    // Send handshake
    let our_id = our_identity.public_id();
    send_handshake(&mut socket, &our_id)?;

    // Create exchange message
    // Convert exchange public key to fixed-size array
    let exchange_key: [u8; 32] = our_identity.exchange_public_key()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid exchange key length"))?;

    let exchange_msg = ExchangeMessage::new(
        our_identity.signing_public_key(),
        &exchange_key,
        our_identity.display_name(),
    );

    // Create encrypted update (using exchange message as ciphertext)
    let update = EncryptedUpdate {
        recipient_id: recipient_id.to_string(),
        sender_id: our_id.clone(),
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

/// Starts a contact exchange by generating a QR code.
pub fn start(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get our identity
    let identity = wb.identity().ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Generate exchange QR
    let qr = ExchangeQR::generate(identity);
    let qr_data = qr.to_data_string();
    let qr_image = qr.to_qr_image_string();

    // Display
    display::info("Share this with another WebBook user:");
    println!();
    println!("{}", qr_image);
    println!();
    println!("Or share this data string:");
    println!("  {}", qr_data);
    println!();

    display::info("After they complete the exchange, run 'webbook sync' to receive their info.");

    Ok(())
}

/// Completes a contact exchange with received data.
pub fn complete(config: &CliConfig, data: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Parse the exchange QR data
    let qr = ExchangeQR::from_data_string(data)?;

    // Check if expired
    if qr.is_expired() {
        bail!("This exchange QR code has expired. Ask them to generate a new one.");
    }

    // Get their public key
    let their_public_key = qr.public_key();
    let their_public_id = hex::encode(their_public_key);

    // Check if we already have this contact
    if wb.get_contact(&their_public_id)?.is_some() {
        display::warning("You already have this contact.");
        return Ok(());
    }

    // For now, we create a contact with a generated shared secret
    // In a real implementation, this would use the X3DH session
    let shared_secret = SymmetricKey::generate();

    // Create a placeholder contact
    // The real name will be received via sync
    let their_card = webbook_core::ContactCard::new("New Contact");

    let contact = Contact::from_exchange(
        *their_public_key,
        their_card,
        shared_secret,
    );

    // Add the contact
    wb.add_contact(contact)?;

    // Get our identity for sending exchange message
    let identity = wb.identity().ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Send exchange message via relay
    println!("Sending exchange request via relay...");
    match send_exchange_message(config, identity, &their_public_id) {
        Ok(()) => {
            display::success("Exchange request sent");
        }
        Err(e) => {
            display::warning(&format!("Could not send via relay: {}", e));
            display::info("The contact has been added locally.");
            display::info("Ask them to run 'webbook sync' or share your QR code manually.");
        }
    }

    println!();
    display::success(&format!("Contact added (ID: {}...)", &their_public_id[..16]));
    display::info("They need to run 'webbook sync' to see your contact request.");
    display::info("You should also run 'webbook sync' to receive their card updates.");

    Ok(())
}

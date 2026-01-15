//! Exchange Command
//!
//! Generate and complete contact exchanges.

use std::fs;

use anyhow::{bail, Result};
use webbook_core::{WebBook, WebBookConfig, Contact, SymmetricKey, Identity, IdentityBackup};
use webbook_core::network::MockTransport;
use webbook_core::exchange::ExchangeQR;

use crate::config::CliConfig;
use crate::display;

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

    display::info("After they scan, they can exchange contacts with:");
    println!("  webbook exchange complete <data>");

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

    // For now, we create a contact with a generated shared secret
    // In a real implementation, this would use the X3DH session
    let shared_secret = SymmetricKey::generate();

    // Get our card to show what we're sharing
    let _our_card = wb.own_card()?.ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    // Create a placeholder contact
    // In a real implementation, this would receive their card through the exchange
    let their_card = webbook_core::ContactCard::new("New Contact");

    let contact = Contact::from_exchange(
        *their_public_key,
        their_card,
        shared_secret,
    );

    let contact_id = contact.id().to_string();

    // Add the contact
    wb.add_contact(contact)?;

    display::success("Contact exchange started");
    println!("  Contact ID: {}", contact_id);
    println!();

    display::warning("Note: Full exchange requires both parties to complete the protocol.");
    display::info("The contact has been added but card sync will happen via relay.");

    // Generate our response
    let identity = wb.identity().ok_or_else(|| anyhow::anyhow!("No identity found"))?;
    let our_qr = ExchangeQR::generate(identity);
    let our_data = our_qr.to_data_string();

    println!();
    display::info("Share your response with them:");
    println!("  {}", our_data);

    Ok(())
}

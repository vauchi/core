//! Card Command
//!
//! Manage your contact card.

use std::fs;

use anyhow::{bail, Result};
use webbook_core::{WebBook, WebBookConfig, ContactField, FieldType, Identity, IdentityBackup};
use webbook_core::network::MockTransport;

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

/// Parses a field type string.
fn parse_field_type(s: &str) -> Result<FieldType> {
    match s.to_lowercase().as_str() {
        "email" | "mail" => Ok(FieldType::Email),
        "phone" | "tel" | "telephone" => Ok(FieldType::Phone),
        "website" | "web" | "url" => Ok(FieldType::Website),
        "address" | "addr" | "home" => Ok(FieldType::Address),
        "social" | "twitter" | "instagram" | "linkedin" => Ok(FieldType::Social),
        "custom" | "other" | "note" => Ok(FieldType::Custom),
        _ => bail!("Unknown field type: {}. Use: email, phone, website, address, social, custom", s),
    }
}

/// Shows the current contact card.
pub fn show(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    match wb.own_card()? {
        Some(card) => {
            display::display_card(&card);
        }
        None => {
            display::warning("No contact card found. Create one with 'webbook init'.");
        }
    }

    Ok(())
}

/// Adds a field to the contact card.
pub fn add(config: &CliConfig, field_type: &str, label: &str, value: &str) -> Result<()> {
    let wb = open_webbook(config)?;
    let ft = parse_field_type(field_type)?;

    // Get old card for delta propagation
    let old_card = wb.own_card()?.ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    let field = ContactField::new(ft, label, value);
    wb.add_own_field(field)?;

    display::success(&format!("Added {} field '{}'", field_type, label));

    // Propagate update to contacts
    let new_card = wb.own_card()?.unwrap();
    let queued = wb.propagate_card_update(&old_card, &new_card)?;
    if queued > 0 {
        display::info(&format!("Update queued to {} contact(s)", queued));
    }

    Ok(())
}

/// Removes a field from the contact card.
pub fn remove(config: &CliConfig, label: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get old card for delta propagation
    let old_card = wb.own_card()?.ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    if wb.remove_own_field(label)? {
        display::success(&format!("Removed field '{}'", label));

        // Propagate update to contacts
        let new_card = wb.own_card()?.unwrap();
        let queued = wb.propagate_card_update(&old_card, &new_card)?;
        if queued > 0 {
            display::info(&format!("Update queued to {} contact(s)", queued));
        }
    } else {
        display::warning(&format!("Field '{}' not found", label));
    }

    Ok(())
}

/// Edits a field value.
pub fn edit(config: &CliConfig, label: &str, value: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get current card (also serves as old card for delta)
    let old_card = wb.own_card()?.ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    // Find the field
    let field = old_card.fields().iter().find(|f| f.label() == label);

    match field {
        Some(f) => {
            // Remove old and add new
            wb.remove_own_field(label)?;
            let new_field = ContactField::new(f.field_type(), label, value);
            wb.add_own_field(new_field)?;

            display::success(&format!("Updated field '{}'", label));

            // Propagate update to contacts
            let new_card = wb.own_card()?.unwrap();
            let queued = wb.propagate_card_update(&old_card, &new_card)?;
            if queued > 0 {
                display::info(&format!("Update queued to {} contact(s)", queued));
            }
        }
        None => {
            display::warning(&format!("Field '{}' not found", label));
        }
    }

    Ok(())
}

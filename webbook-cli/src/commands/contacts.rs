//! Contacts Command
//!
//! List, view, and manage contacts.

use std::fs;

use anyhow::{bail, Result};
use webbook_core::{WebBook, WebBookConfig, Identity, IdentityBackup};
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

/// Lists all contacts.
pub fn list(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;
    let contacts = wb.list_contacts()?;

    if contacts.is_empty() {
        display::info("No contacts yet. Exchange with someone using:");
        println!("  webbook exchange start");
        return Ok(());
    }

    println!();
    println!("Contacts ({}):", contacts.len());
    println!();

    for (i, contact) in contacts.iter().enumerate() {
        display::display_contact_summary(contact, i + 1);
    }

    println!();

    Ok(())
}

/// Shows details for a specific contact.
pub fn show(config: &CliConfig, id: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Try to find by ID first, then by name
    let contact = wb.get_contact(id)?
        .or_else(|| {
            // Search by name
            wb.search_contacts(id).ok()
                .and_then(|results| results.into_iter().next())
        });

    match contact {
        Some(c) => {
            display::display_contact_details(&c);
        }
        None => {
            display::warning(&format!("Contact '{}' not found", id));
        }
    }

    Ok(())
}

/// Searches contacts by query.
pub fn search(config: &CliConfig, query: &str) -> Result<()> {
    let wb = open_webbook(config)?;
    let results = wb.search_contacts(query)?;

    if results.is_empty() {
        display::info(&format!("No contacts matching '{}'", query));
        return Ok(());
    }

    println!();
    println!("Search results for '{}':", query);
    println!();

    for (i, contact) in results.iter().enumerate() {
        display::display_contact_summary(contact, i + 1);
    }

    println!();

    Ok(())
}

/// Removes a contact.
pub fn remove(config: &CliConfig, id: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get contact name before removing
    let contact = wb.get_contact(id)?;
    let name = contact.as_ref().map(|c| c.display_name().to_string());

    if wb.remove_contact(id)? {
        display::success(&format!(
            "Removed contact: {}",
            name.unwrap_or_else(|| id.to_string())
        ));
    } else {
        display::warning(&format!("Contact '{}' not found", id));
    }

    Ok(())
}

/// Marks a contact's fingerprint as verified.
pub fn verify(config: &CliConfig, id: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get contact first
    let contact = wb.get_contact(id)?
        .ok_or_else(|| anyhow::anyhow!("Contact '{}' not found", id))?;

    let name = contact.display_name().to_string();

    if contact.is_fingerprint_verified() {
        display::info(&format!("{} is already verified", name));
        return Ok(());
    }

    wb.verify_contact_fingerprint(id)?;
    display::success(&format!("Verified fingerprint for {}", name));

    Ok(())
}

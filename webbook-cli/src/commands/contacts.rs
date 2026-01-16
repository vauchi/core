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

    let wb_config = WebBookConfig::with_storage_path(config.storage_path())
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

/// Helper to find contact by ID or name
fn find_contact(wb: &WebBook<MockTransport>, id_or_name: &str) -> Result<webbook_core::Contact> {
    // Try to find by ID first, then by name
    let contact = wb.get_contact(id_or_name)?
        .or_else(|| {
            // Search by name
            wb.search_contacts(id_or_name).ok()
                .and_then(|results| results.into_iter().next())
        })
        .ok_or_else(|| anyhow::anyhow!("Contact '{}' not found", id_or_name))?;
    Ok(contact)
}

/// Helper to find field ID by label in own card
fn find_field_id(wb: &WebBook<MockTransport>, label: &str) -> Result<String> {
    let card = wb.own_card()?
        .ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    let field = card.fields().iter()
        .find(|f| f.label() == label)
        .ok_or_else(|| anyhow::anyhow!("Field '{}' not found in your card", label))?;

    Ok(field.id().to_string())
}

/// Hides a field from a specific contact.
pub fn hide_field(config: &CliConfig, contact_id_or_name: &str, field_label: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Find contact
    let mut contact = find_contact(&wb, contact_id_or_name)?;
    let contact_name = contact.display_name().to_string();

    // Find field ID by label
    let field_id = find_field_id(&wb, field_label)?;

    // Set visibility to nobody for this field
    contact.visibility_rules_mut().set_nobody(&field_id);
    wb.update_contact(&contact)?;

    display::success(&format!(
        "Hidden '{}' field from {}",
        field_label, contact_name
    ));
    display::info("Changes will take effect on next sync.");

    Ok(())
}

/// Shows (unhides) a field to a specific contact.
pub fn unhide_field(config: &CliConfig, contact_id_or_name: &str, field_label: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Find contact
    let mut contact = find_contact(&wb, contact_id_or_name)?;
    let contact_name = contact.display_name().to_string();

    // Find field ID by label
    let field_id = find_field_id(&wb, field_label)?;

    // Set visibility to everyone for this field
    contact.visibility_rules_mut().set_everyone(&field_id);
    wb.update_contact(&contact)?;

    display::success(&format!(
        "'{}' field is now visible to {}",
        field_label, contact_name
    ));
    display::info("Changes will take effect on next sync.");

    Ok(())
}

/// Shows visibility rules for a specific contact.
pub fn show_visibility(config: &CliConfig, contact_id_or_name: &str) -> Result<()> {
    use webbook_core::FieldVisibility;

    let wb = open_webbook(config)?;

    // Find contact
    let contact = find_contact(&wb, contact_id_or_name)?;
    let contact_name = contact.display_name().to_string();

    // Get our card fields
    let card = wb.own_card()?
        .ok_or_else(|| anyhow::anyhow!("No contact card found"))?;

    println!();
    println!("Visibility rules for {}:", contact_name);
    println!();

    if card.fields().is_empty() {
        display::info("No fields in your card.");
        return Ok(());
    }

    let rules = contact.visibility_rules();
    let mut has_custom_rules = false;

    for field in card.fields() {
        let visibility = rules.get(field.id());
        let status = match visibility {
            FieldVisibility::Everyone => "✓ visible",
            FieldVisibility::Nobody => "✗ hidden",
            FieldVisibility::Contacts(allowed) => {
                if allowed.contains(&contact.id().to_string()) {
                    "✓ visible (restricted)"
                } else {
                    "✗ hidden (restricted)"
                }
            }
        };

        if !matches!(visibility, FieldVisibility::Everyone) {
            has_custom_rules = true;
        }

        println!("  {} {}: {}", status, field.label(), field.value());
    }

    if !has_custom_rules {
        println!();
        display::info("All fields are visible to this contact (default).");
    }

    println!();

    Ok(())
}

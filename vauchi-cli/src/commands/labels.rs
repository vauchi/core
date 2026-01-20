//! Visibility Labels Commands
//!
//! Manage visibility labels for organizing contacts.

use anyhow::{anyhow, bail, Result};
use vauchi_core::Storage;

use crate::config::CliConfig;
use crate::display;

/// Opens storage from the config.
fn open_storage(config: &CliConfig) -> Result<Storage> {
    if !config.is_initialized() {
        bail!("Vauchi not initialized. Run 'vauchi init <name>' first.");
    }

    let storage = Storage::open(config.storage_path(), config.storage_key()?)?;
    Ok(storage)
}

/// List all labels.
pub fn list(config: &CliConfig) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    if labels.is_empty() {
        display::info("No labels defined. Create one with 'vauchi labels create <name>'");
        display::info(&format!(
            "Suggested labels: {}",
            vauchi_core::SUGGESTED_LABELS.join(", ")
        ));
        return Ok(());
    }

    println!("Visibility Labels:");
    println!();
    for label in labels {
        let contacts = label.contact_count();
        let fields = label.visible_fields().len();
        println!(
            "  {} ({})",
            label.name(),
            label.id().chars().take(8).collect::<String>()
        );
        println!("    Contacts: {}, Visible fields: {}", contacts, fields);
    }

    Ok(())
}

/// Create a new label.
pub fn create(config: &CliConfig, name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let label = storage.create_label(name)?;

    display::success(&format!(
        "Created label '{}' (ID: {})",
        label.name(),
        label.id()
    ));
    Ok(())
}

/// Show label details.
pub fn show(config: &CliConfig, label_name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    println!("Label: {}", label.name());
    println!("ID: {}", label.id());
    println!("Created: {}", format_timestamp(label.created_at()));
    println!("Modified: {}", format_timestamp(label.modified_at()));
    println!();

    // Show contacts
    let contact_ids: Vec<_> = label.contacts().iter().cloned().collect();
    if contact_ids.is_empty() {
        println!("Contacts: (none)");
    } else {
        println!("Contacts:");
        let all_contacts = storage.list_contacts()?;
        for contact_id in &contact_ids {
            let name = all_contacts
                .iter()
                .find(|c| c.id() == contact_id)
                .map(|c| c.display_name())
                .unwrap_or("(unknown)");
            println!("  - {} ({})", name, &contact_id[..8]);
        }
    }
    println!();

    // Show visible fields
    let field_ids: Vec<_> = label.visible_fields().iter().cloned().collect();
    if field_ids.is_empty() {
        println!("Visible fields: (none - contacts see default visibility)");
    } else {
        println!("Visible fields:");
        if let Some(card) = storage.load_own_card()? {
            for field_id in &field_ids {
                let label_name = card
                    .fields()
                    .iter()
                    .find(|f| f.id() == field_id)
                    .map(|f| f.label())
                    .unwrap_or("(unknown)");
                println!("  - {}", label_name);
            }
        }
    }

    Ok(())
}

/// Rename a label.
pub fn rename(config: &CliConfig, label_name: &str, new_name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    storage.rename_label(label.id(), new_name)?;
    display::success(&format!("Renamed label to '{}'", new_name));
    Ok(())
}

/// Delete a label.
pub fn delete(config: &CliConfig, label_name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    let name = label.name().to_string();
    storage.delete_label(label.id())?;
    display::success(&format!("Deleted label '{}'", name));
    Ok(())
}

/// Add a contact to a label.
pub fn add_contact(config: &CliConfig, label_name: &str, contact_name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;
    let contacts = storage.list_contacts()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    let contact = contacts
        .iter()
        .find(|c| {
            c.display_name().eq_ignore_ascii_case(contact_name) || c.id().starts_with(contact_name)
        })
        .ok_or_else(|| anyhow!("Contact not found: {}", contact_name))?;

    storage.add_contact_to_label(label.id(), contact.id())?;
    display::success(&format!(
        "Added '{}' to label '{}'",
        contact.display_name(),
        label.name()
    ));
    Ok(())
}

/// Remove a contact from a label.
pub fn remove_contact(config: &CliConfig, label_name: &str, contact_name: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;
    let contacts = storage.list_contacts()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    let contact = contacts
        .iter()
        .find(|c| {
            c.display_name().eq_ignore_ascii_case(contact_name) || c.id().starts_with(contact_name)
        })
        .ok_or_else(|| anyhow!("Contact not found: {}", contact_name))?;

    storage.remove_contact_from_label(label.id(), contact.id())?;
    display::success(&format!(
        "Removed '{}' from label '{}'",
        contact.display_name(),
        label.name()
    ));
    Ok(())
}

/// Show a field to contacts in a label.
pub fn show_field(config: &CliConfig, label_name: &str, field_label: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    let card = storage
        .load_own_card()?
        .ok_or_else(|| anyhow!("No contact card found"))?;

    let field = card
        .fields()
        .iter()
        .find(|f| f.label().eq_ignore_ascii_case(field_label))
        .ok_or_else(|| anyhow!("Field not found: {}", field_label))?;

    storage.set_label_field_visibility(label.id(), field.id(), true)?;
    display::success(&format!(
        "Field '{}' is now visible to contacts in '{}'",
        field.label(),
        label.name()
    ));
    Ok(())
}

/// Hide a field from contacts in a label.
pub fn hide_field(config: &CliConfig, label_name: &str, field_label: &str) -> Result<()> {
    let storage = open_storage(config)?;
    let labels = storage.load_all_labels()?;

    let label = labels
        .iter()
        .find(|l| l.name().eq_ignore_ascii_case(label_name) || l.id().starts_with(label_name))
        .ok_or_else(|| anyhow!("Label not found: {}", label_name))?;

    let card = storage
        .load_own_card()?
        .ok_or_else(|| anyhow!("No contact card found"))?;

    let field = card
        .fields()
        .iter()
        .find(|f| f.label().eq_ignore_ascii_case(field_label))
        .ok_or_else(|| anyhow!("Field not found: {}", field_label))?;

    storage.set_label_field_visibility(label.id(), field.id(), false)?;
    display::success(&format!(
        "Field '{}' is now hidden from contacts in '{}'",
        field.label(),
        label.name()
    ));
    Ok(())
}

fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let dt = UNIX_EPOCH + Duration::from_secs(ts);
    let now = SystemTime::now();
    let elapsed = now
        .duration_since(dt)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();

    if elapsed < 60 {
        "just now".to_string()
    } else if elapsed < 3600 {
        format!("{} minutes ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{} hours ago", elapsed / 3600)
    } else {
        format!("{} days ago", elapsed / 86400)
    }
}

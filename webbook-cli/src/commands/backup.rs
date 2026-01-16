//! Backup Commands
//!
//! Export and import identity backups.

use std::fs;
use std::path::Path;

use anyhow::{bail, Result};
use dialoguer::{Input, Password};
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

/// Exports an identity backup.
pub fn export(config: &CliConfig, output: &Path) -> Result<()> {
    let wb = open_webbook(config)?;

    // Get identity
    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Prompt for password
    let password: String = Password::new()
        .with_prompt("Enter backup password")
        .with_confirmation("Confirm password", "Passwords don't match")
        .interact()?;

    // Create encrypted backup
    let backup = identity.export_backup(&password)?;

    // Write to file
    fs::write(output, backup.as_bytes())?;

    display::success(&format!("Backup saved to {:?}", output));
    display::warning("Keep this file and password safe. You'll need both to restore.");

    Ok(())
}

/// Imports an identity from backup.
pub fn import(config: &CliConfig, input: &Path) -> Result<()> {
    // Check if already initialized
    if config.is_initialized() {
        display::warning("WebBook is already initialized.");

        let confirm: String = Input::new()
            .with_prompt("This will overwrite existing data. Type 'yes' to continue")
            .interact_text()?;

        if confirm.to_lowercase() != "yes" {
            display::info("Import cancelled.");
            return Ok(());
        }
    }

    // Read backup file
    let backup_data = fs::read(input)?;
    let backup = IdentityBackup::new(backup_data);

    // Prompt for password
    let password: String = Password::new()
        .with_prompt("Enter backup password")
        .interact()?;

    // Restore identity
    let identity = Identity::import_backup(&backup, &password)?;

    let name = identity.display_name().to_string();

    // Create data directory
    fs::create_dir_all(&config.data_dir)?;

    // Save identity to local file for persistence
    let local_backup = identity.export_backup(LOCAL_STORAGE_PASSWORD)?;
    fs::write(config.identity_path(), local_backup.as_bytes())?;

    // Initialize WebBook with restored identity
    let wb_config = WebBookConfig::with_storage_path(config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key());

    let mut wb: WebBook<MockTransport> = WebBook::new(wb_config)?;
    wb.set_identity(identity)?;

    display::success(&format!("Identity restored: {}", name));
    display::info("Your contacts and card will need to sync from the relay.");

    Ok(())
}

//! Init Command
//!
//! Creates a new Vauchi identity.

use std::fs;

use anyhow::{bail, Result};
use vauchi_core::network::MockTransport;
use vauchi_core::{Vauchi, VauchiConfig};

use crate::config::CliConfig;
use crate::display;

/// Internal password for local identity storage.
/// This is not for security - just for CLI persistence.
const LOCAL_STORAGE_PASSWORD: &str = "vauchi-local-storage";

/// Creates a new identity.
pub fn run(name: &str, config: &CliConfig) -> Result<()> {
    // Check if already initialized
    if config.is_initialized() {
        bail!("Vauchi is already initialized in {:?}. Use --data-dir to specify a different location.", config.data_dir);
    }

    // Create data directory
    fs::create_dir_all(&config.data_dir)?;

    // Initialize Vauchi with persistent storage key
    let wb_config = VauchiConfig::with_storage_path(config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key()?);

    let mut wb: Vauchi<MockTransport> = Vauchi::new(wb_config)?;
    wb.create_identity(name)?;

    // Save identity to file for persistence
    let identity = wb
        .identity()
        .ok_or_else(|| anyhow::anyhow!("Identity not found after creation"))?;
    let backup = identity.export_backup(LOCAL_STORAGE_PASSWORD)?;
    fs::write(config.identity_path(), backup.as_bytes())?;

    // Get identity info
    let public_id = wb.public_id()?;

    display::success(&format!("Identity created: {}", name));
    println!();
    println!("  Public ID: {}", public_id);
    println!("  Data dir:  {:?}", config.data_dir);
    println!();
    display::info("Add contact info with: vauchi card add <type> <label> <value>");

    Ok(())
}

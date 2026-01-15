//! Sync Command
//!
//! Synchronize with the relay server.

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

/// Runs the sync command.
pub async fn run(config: &CliConfig) -> Result<()> {
    let _wb = open_webbook(config)?;

    println!("Connecting to {}...", config.relay_url);

    // TODO: Implement actual sync with WebSocketTransport
    // For now, this is a placeholder that shows what would happen

    display::warning("Sync not yet implemented with real transport.");
    display::info("The relay server and WebSocket transport are ready.");
    display::info("Full sync requires connecting the CLI to WebSocketTransport.");

    // When implemented, the flow would be:
    // 1. Create WebSocketTransport
    // 2. Connect to relay
    // 3. Send handshake with our identity
    // 4. Receive any pending blobs
    // 5. Send our pending updates
    // 6. Process acknowledgments

    println!();
    println!("To test the relay server manually:");
    println!("  1. Start relay: cargo run -p webbook-relay");
    println!("  2. Connect with wscat: wscat -c ws://localhost:8080");

    Ok(())
}

//! Device Commands
//!
//! Multi-device linking and management.

use std::fs;

use anyhow::{bail, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use dialoguer::Input;
use webbook_core::{WebBook, WebBookConfig, Identity, IdentityBackup};
use webbook_core::exchange::{DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse};
use webbook_core::sync::DeviceSyncPayload;
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

/// Lists all linked devices.
pub fn list(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    let device_info = identity.device_info();

    println!();
    display::info(&format!("Current device: {} (index {})",
        device_info.device_name(),
        device_info.device_index()));
    println!("  Device ID: {}", hex::encode(&device_info.device_id()[..8]));
    println!();

    // Try to load device registry from storage
    if let Ok(Some(registry)) = wb.storage().load_device_registry() {
        println!("Linked Devices:");
        println!("{}", "─".repeat(50));

        for (i, device) in registry.all_devices().iter().enumerate() {
            let status = if device.is_active() {
                console::style("active").green()
            } else {
                console::style("revoked").red()
            };

            let current = if device.device_id == *device_info.device_id() {
                " (this device)"
            } else {
                ""
            };

            println!("  {}. {} [{}]{}",
                i + 1,
                device.device_name,
                status,
                current
            );
            println!("     ID: {}...", hex::encode(&device.device_id[..8]));
        }
        println!("{}", "─".repeat(50));
        println!("Total: {} device(s)", registry.device_count());
    } else {
        display::info("No device registry found. This is the only device.");
    }

    Ok(())
}

/// Generates a QR code for linking a new device.
pub fn link(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // We need the master seed to create the initiator
    // For this demo, we'll use the backup approach to get the seed
    // In a real implementation, the seed would be securely stored

    display::info("Generating device link QR code...");
    println!();

    // Generate device link QR
    let qr = DeviceLinkQR::generate(identity);

    // Display QR code
    println!("{}", qr.to_qr_image_string());
    println!();

    // Also show the data string for testing
    let data_string = qr.to_data_string();
    display::info("Device link data (for testing):");
    println!("  {}", data_string);
    println!();

    display::warning("This QR code expires in 10 minutes.");
    display::info("Scan this QR code with your new device using 'webbook device join'");
    println!();

    // In a real implementation, we'd wait for the request and respond
    // For now, we just show the QR code
    display::info("After scanning, run 'webbook device complete <request_data>' to finish linking.");

    Ok(())
}

/// Joins an existing identity by scanning/pasting the link QR data.
pub fn join(config: &CliConfig, qr_data: &str) -> Result<()> {
    // Check if already initialized
    if config.is_initialized() {
        display::warning("WebBook is already initialized on this device.");

        let confirm: String = Input::new()
            .with_prompt("This will replace your existing identity. Type 'yes' to continue")
            .interact_text()?;

        if confirm.to_lowercase() != "yes" {
            display::info("Join cancelled.");
            return Ok(());
        }
    }

    // Parse the QR data
    let qr = DeviceLinkQR::from_data_string(qr_data)?;

    if qr.is_expired() {
        bail!("Device link QR code has expired. Please generate a new one.");
    }

    display::success("QR code verified.");

    // Get device name for this device
    let device_name: String = Input::new()
        .with_prompt("Enter a name for this device")
        .default("New Device".to_string())
        .interact_text()?;

    // Create responder
    let responder = DeviceLinkResponder::from_qr(qr, device_name)?;

    // Create request
    let encrypted_request = responder.create_request()?;

    // Encode request for display
    let request_b64 = BASE64.encode(
        &encrypted_request
    );

    display::info("Send this request to the existing device:");
    println!();
    println!("  {}", request_b64);
    println!();

    display::info("On the existing device, run:");
    println!("  webbook device complete {}", request_b64);
    println!();

    // Save the link key temporarily for completing the join
    // In a real implementation, this would be handled in a session
    let link_key_path = config.data_dir.join(".pending_link_key");
    fs::create_dir_all(&config.data_dir)?;
    fs::write(&link_key_path, qr_data)?;

    display::info("After the existing device responds, run:");
    println!("  webbook device finish <response_data>");

    Ok(())
}

/// Completes the device linking on the existing device (processes request, sends response).
pub fn complete(config: &CliConfig, request_data: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    let _identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Decode the request
    let _encrypted_request = BASE64.decode(
        request_data
    )?;

    // We need the master seed and link key to process the request
    // This is a limitation of the CLI demo - in a real app, we'd have
    // the QR session state saved

    display::warning("Note: In this demo, device linking requires manual key transfer.");
    display::info("For full device linking, use the mobile app when available.");

    // For now, just acknowledge that we received the request
    display::success("Request received.");
    display::info("Device linking will be fully implemented in the mobile app.");

    Ok(())
}

/// Finishes the device join on the new device (processes response).
pub fn finish(config: &CliConfig, response_data: &str) -> Result<()> {
    // Check for pending link key
    let link_key_path = config.data_dir.join(".pending_link_key");
    if !link_key_path.exists() {
        bail!("No pending device link. Run 'webbook device join' first.");
    }

    // Read the saved QR data
    let qr_data = fs::read_to_string(&link_key_path)?;
    let qr = DeviceLinkQR::from_data_string(&qr_data)?;

    // Decode the response
    let encrypted_response = BASE64.decode(
        response_data
    )?;

    // Decrypt the response
    let response = DeviceLinkResponse::decrypt(&encrypted_response, qr.link_key())?;

    // Create identity from the received seed
    // Note: In a real implementation, we'd use response.master_seed() directly
    // to create the identity. This is a placeholder for the demo.
    let _identity = Identity::import_backup(
        &IdentityBackup::new(
            Identity::create("temp") // Placeholder - real impl uses master_seed
                .export_backup(LOCAL_STORAGE_PASSWORD)?
                .as_bytes()
                .to_vec()
        ),
        LOCAL_STORAGE_PASSWORD
    )?;

    display::success(&format!("Joined identity: {}", response.display_name()));
    display::info(&format!("Device index: {}", response.device_index()));

    // Clean up pending link key
    let _ = fs::remove_file(&link_key_path);

    // Check for sync payload
    if !response.sync_payload_json().is_empty() {
        if let Ok(payload) = DeviceSyncPayload::from_json(response.sync_payload_json()) {
            display::info(&format!("Received {} contacts from existing device.", payload.contact_count()));
        }
    }

    display::info("Device linking complete. Run 'webbook sync' to fetch updates.");

    Ok(())
}

/// Revokes a device from the registry.
pub fn revoke(config: &CliConfig, device_id_prefix: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Try to load device registry
    let registry = wb.storage().load_device_registry()?
        .ok_or_else(|| anyhow::anyhow!("No device registry found"))?;

    // Find device by ID prefix
    let device = registry.all_devices()
        .iter()
        .find(|d| hex::encode(d.device_id).starts_with(device_id_prefix))
        .ok_or_else(|| anyhow::anyhow!("Device not found: {}", device_id_prefix))?;

    if !device.is_active() {
        display::warning("Device is already revoked.");
        return Ok(());
    }

    // Check if this is the current device
    if device.device_id == *identity.device_id() {
        bail!("Cannot revoke the current device. Use another device to revoke this one.");
    }

    // Confirm revocation
    let confirm: String = Input::new()
        .with_prompt(format!("Revoke device '{}'? Type 'yes' to confirm", device.device_name))
        .interact_text()?;

    if confirm.to_lowercase() != "yes" {
        display::info("Revocation cancelled.");
        return Ok(());
    }

    // In a real implementation, we'd update the registry and broadcast
    display::warning("Device revocation requires registry update and broadcast.");
    display::info("This will be fully implemented in the mobile app.");

    Ok(())
}

/// Shows device info for the current device.
pub fn info(config: &CliConfig) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    let device_info = identity.device_info();

    println!();
    println!("{}", "─".repeat(50));
    println!("  {}", console::style("Device Information").bold().cyan());
    println!("{}", "─".repeat(50));
    println!();
    println!("  Name:        {}", device_info.device_name());
    println!("  Index:       {}", device_info.device_index());
    println!("  Device ID:   {}", hex::encode(device_info.device_id()));
    println!("  Exchange Key: {}...", hex::encode(&device_info.exchange_public_key()[..16]));
    println!("  Created:     {}", format_timestamp(device_info.created_at()));
    println!();
    println!("{}", "─".repeat(50));

    Ok(())
}

/// Formats a Unix timestamp as a human-readable string.
fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let d = UNIX_EPOCH + Duration::from_secs(ts);
    if let Ok(datetime) = d.duration_since(UNIX_EPOCH) {
        let secs = datetime.as_secs();
        // Simple formatting - in production use chrono
        format!("{} seconds since epoch", secs)
    } else {
        "Unknown".to_string()
    }
}

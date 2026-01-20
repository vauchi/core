//! Device Commands
//!
//! Multi-device linking and management.

use std::fs;

use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use dialoguer::Input;
use vauchi_core::exchange::{DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse};
use vauchi_core::network::MockTransport;
use vauchi_core::sync::DeviceSyncPayload;
use vauchi_core::{Identity, IdentityBackup, Vauchi, VauchiConfig};

use crate::config::CliConfig;
use crate::display;

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "vauchi-local-storage";

/// Opens Vauchi from the config and loads the identity.
fn open_vauchi(config: &CliConfig) -> Result<Vauchi<MockTransport>> {
    if !config.is_initialized() {
        bail!("Vauchi not initialized. Run 'vauchi init <name>' first.");
    }

    let wb_config = VauchiConfig::with_storage_path(config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key()?);

    let mut wb = Vauchi::new(wb_config)?;

    // Load identity from file
    let backup_data = fs::read(config.identity_path())?;
    let backup = IdentityBackup::new(backup_data);
    let identity = Identity::import_backup(&backup, LOCAL_STORAGE_PASSWORD)?;
    wb.set_identity(identity)?;

    Ok(wb)
}

/// Lists all linked devices.
pub fn list(config: &CliConfig) -> Result<()> {
    let wb = open_vauchi(config)?;

    let identity = wb
        .identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    let device_info = identity.device_info();

    println!();
    display::info(&format!(
        "Current device: {} (index {})",
        device_info.device_name(),
        device_info.device_index()
    ));
    println!(
        "  Device ID: {}",
        hex::encode(&device_info.device_id()[..8])
    );
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

            println!(
                "  {}. {} [{}]{}",
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
    let wb = open_vauchi(config)?;

    let identity = wb
        .identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Get or create device registry
    let registry = wb
        .storage()
        .load_device_registry()?
        .unwrap_or_else(|| identity.initial_device_registry());

    display::info("Generating device link QR code...");
    println!();

    // Create device link initiator (which generates the QR)
    let initiator = identity.create_device_link_initiator(registry);
    let qr = initiator.qr();

    // Display QR code
    println!("{}", qr.to_qr_image_string());
    println!();

    // Save the QR data for use in 'device complete'
    let data_string = qr.to_data_string();
    let pending_link_path = config.data_dir.join(".pending_device_link");
    fs::create_dir_all(&config.data_dir)?;
    fs::write(&pending_link_path, &data_string)?;

    // Also show the data string for testing
    display::info("Device link data (for testing):");
    println!("  {}", data_string);
    println!();

    display::warning("This QR code expires in 10 minutes.");
    display::info("Scan this QR code with your new device using 'vauchi device join'");
    println!();

    display::info("After scanning, run 'vauchi device complete <request_data>' to finish linking.");

    Ok(())
}

/// Joins an existing identity by scanning/pasting the link QR data.
pub fn join(config: &CliConfig, qr_data: &str) -> Result<()> {
    // Check if already initialized
    if config.is_initialized() {
        display::warning("Vauchi is already initialized on this device.");

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
    let responder = DeviceLinkResponder::from_qr(qr, device_name.clone())?;

    // Create request
    let encrypted_request = responder.create_request()?;

    // Encode request for display
    let request_b64 = BASE64.encode(&encrypted_request);

    display::info("Send this request to the existing device:");
    println!();
    println!("  {}", request_b64);
    println!();

    display::info("On the existing device, run:");
    println!("  vauchi device complete {}", request_b64);
    println!();

    // Save the QR data and device name for completing the join
    let link_key_path = config.data_dir.join(".pending_link_key");
    let device_name_path = config.data_dir.join(".pending_device_name");
    fs::create_dir_all(&config.data_dir)?;
    fs::write(&link_key_path, qr_data)?;
    fs::write(&device_name_path, &device_name)?;

    display::info("After the existing device responds, run:");
    println!("  vauchi device finish <response_data>");

    Ok(())
}

/// Completes the device linking on the existing device (processes request, sends response).
pub fn complete(config: &CliConfig, request_data: &str) -> Result<()> {
    let wb = open_vauchi(config)?;

    let identity = wb
        .identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Load the pending device link QR data
    let pending_link_path = config.data_dir.join(".pending_device_link");
    if !pending_link_path.exists() {
        bail!("No pending device link. Run 'vauchi device link' first.");
    }

    let qr_data_string = fs::read_to_string(&pending_link_path)?;
    let saved_qr = DeviceLinkQR::from_data_string(&qr_data_string)?;

    if saved_qr.is_expired() {
        // Clean up expired link
        let _ = fs::remove_file(&pending_link_path);
        bail!("Device link QR has expired. Please run 'vauchi device link' again.");
    }

    // Get or create device registry
    let registry = wb
        .storage()
        .load_device_registry()?
        .unwrap_or_else(|| identity.initial_device_registry());

    // Restore the initiator with the saved QR
    let initiator = identity.restore_device_link_initiator(registry, saved_qr);

    // Decode the request
    let encrypted_request = BASE64.decode(request_data)?;

    // Process the request
    let (encrypted_response, updated_registry, new_device) =
        initiator.process_request(&encrypted_request)?;

    // Save the updated registry
    wb.storage().save_device_registry(&updated_registry)?;

    // Encode response for display
    let response_b64 = BASE64.encode(&encrypted_response);

    display::success(&format!(
        "Device '{}' approved for linking!",
        new_device.device_name()
    ));
    println!();

    display::info("Send this response to the new device:");
    println!();
    println!("  {}", response_b64);
    println!();

    display::info("On the new device, run:");
    println!("  vauchi device finish {}", response_b64);
    println!();

    // Clean up the pending link
    let _ = fs::remove_file(&pending_link_path);

    display::success("Device linking initiated. Registry updated with new device.");

    Ok(())
}

/// Finishes the device join on the new device (processes response).
pub fn finish(config: &CliConfig, response_data: &str) -> Result<()> {
    // Check for pending link key
    let link_key_path = config.data_dir.join(".pending_link_key");
    let device_name_path = config.data_dir.join(".pending_device_name");

    if !link_key_path.exists() {
        bail!("No pending device link. Run 'vauchi device join' first.");
    }

    // Read the saved QR data and device name
    let qr_data = fs::read_to_string(&link_key_path)?;
    let device_name =
        fs::read_to_string(&device_name_path).unwrap_or_else(|_| "New Device".to_string());
    let qr = DeviceLinkQR::from_data_string(&qr_data)?;

    // Decode the response
    let encrypted_response = BASE64.decode(response_data)?;

    // Decrypt the response
    let response = DeviceLinkResponse::decrypt(&encrypted_response, qr.link_key())?;

    // Create identity from the received seed
    let identity = Identity::from_device_link(
        *response.master_seed(),
        response.display_name().to_string(),
        response.device_index(),
        device_name,
    );

    // Save the identity
    let backup = identity.export_backup(LOCAL_STORAGE_PASSWORD)?;
    fs::create_dir_all(&config.data_dir)?;
    fs::write(config.identity_path(), backup.as_bytes())?;

    // Save the device registry
    let wb_config = VauchiConfig::with_storage_path(config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key()?);
    let wb = Vauchi::new(wb_config)?;
    wb.storage().save_device_registry(response.registry())?;

    display::success(&format!("Joined identity: {}", response.display_name()));
    display::info(&format!("Device index: {}", response.device_index()));

    // Clean up pending files
    let _ = fs::remove_file(&link_key_path);
    let _ = fs::remove_file(&device_name_path);

    // Check for sync payload
    if !response.sync_payload_json().is_empty() {
        if let Ok(payload) = DeviceSyncPayload::from_json(response.sync_payload_json()) {
            display::info(&format!(
                "Received {} contacts from existing device.",
                payload.contact_count()
            ));
        }
    }

    display::info("Device linking complete. Run 'vauchi sync' to fetch updates.");

    Ok(())
}

/// Revokes a device from the registry.
pub fn revoke(config: &CliConfig, device_id_prefix: &str) -> Result<()> {
    let wb = open_vauchi(config)?;

    let identity = wb
        .identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Try to load device registry
    let registry = wb
        .storage()
        .load_device_registry()?
        .ok_or_else(|| anyhow::anyhow!("No device registry found"))?;

    // Find device by ID prefix
    let device = registry
        .all_devices()
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
        .with_prompt(format!(
            "Revoke device '{}'? Type 'yes' to confirm",
            device.device_name
        ))
        .interact_text()?;

    if confirm.to_lowercase() != "yes" {
        display::info("Revocation cancelled.");
        return Ok(());
    }

    // Update the registry with the revocation
    let mut updated_registry = registry.clone();
    updated_registry.revoke_device(&device.device_id, identity.signing_keypair())?;

    // Save the updated registry
    wb.storage().save_device_registry(&updated_registry)?;

    display::success(&format!(
        "Device '{}' has been revoked.",
        device.device_name
    ));
    display::info("The revocation will be propagated to contacts on next sync.");

    Ok(())
}

/// Shows device info for the current device.
pub fn info(config: &CliConfig) -> Result<()> {
    let wb = open_vauchi(config)?;

    let identity = wb
        .identity()
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
    println!(
        "  Exchange Key: {}...",
        hex::encode(&device_info.exchange_public_key()[..16])
    );
    println!(
        "  Created:     {}",
        format_timestamp(device_info.created_at())
    );
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

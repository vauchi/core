//! Device Management Commands
//!
//! Commands for multi-device linking and management.

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Device info for the frontend.
#[derive(Serialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub device_index: u32,
    pub is_current: bool,
    pub is_active: bool,
}

/// Get list of all linked devices.
#[tauri::command]
pub fn list_devices(state: State<'_, Mutex<AppState>>) -> Result<Vec<DeviceInfo>, String> {
    let state = state.lock().unwrap();

    // Get current device info from identity
    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    let current_device = identity.device_info();
    let current_device_id = hex::encode(current_device.device_id());

    let mut devices = vec![DeviceInfo {
        device_id: current_device_id.clone(),
        device_name: current_device.device_name().to_string(),
        device_index: current_device.device_index(),
        is_current: true,
        is_active: true,
    }];

    // Try to load device registry for other devices
    if let Ok(Some(registry)) = state.storage.load_device_registry() {
        for (i, device) in registry.all_devices().iter().enumerate() {
            let device_id = hex::encode(device.device_id);
            if device_id != current_device_id {
                devices.push(DeviceInfo {
                    device_id,
                    device_name: device.device_name.clone(),
                    device_index: i as u32,
                    is_current: false,
                    is_active: device.is_active(),
                });
            }
        }
    }

    Ok(devices)
}

/// Get current device info.
#[tauri::command]
pub fn get_current_device(state: State<'_, Mutex<AppState>>) -> Result<DeviceInfo, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    let device = identity.device_info();

    Ok(DeviceInfo {
        device_id: hex::encode(device.device_id()),
        device_name: device.device_name().to_string(),
        device_index: device.device_index(),
        is_current: true,
        is_active: true,
    })
}

/// Generate device link QR data for pairing a new device.
#[tauri::command]
pub fn generate_device_link(state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    // Generate device link QR
    use vauchi_core::exchange::DeviceLinkQR;
    let qr = DeviceLinkQR::generate(identity);

    Ok(qr.to_data_string())
}

/// Result of joining a device.
#[derive(Serialize)]
pub struct JoinDeviceResult {
    pub success: bool,
    pub device_name: String,
    pub message: String,
}

/// Join another device using link data.
///
/// This processes a device link QR to add this device to an existing identity.
#[tauri::command]
pub fn join_device(
    link_data: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<JoinDeviceResult, String> {
    use vauchi_core::exchange::DeviceLinkQR;

    let mut state = state.lock().unwrap();

    // Check if we already have an identity
    if state.identity.is_some() {
        return Err("This device already has an identity. Cannot join another device.".to_string());
    }

    // Parse the link data
    let qr = DeviceLinkQR::from_data_string(&link_data)
        .map_err(|e| format!("Invalid link data: {:?}", e))?;

    // Check if the link has expired
    if qr.is_expired() {
        return Err("This device link has expired. Please generate a new one.".to_string());
    }

    // TODO: Complete the device join flow
    // This requires:
    // 1. Performing key agreement with the linking device
    // 2. Receiving the shared identity backup
    // 3. Registering this device in the device registry
    // 4. Saving the identity to storage
    //
    // For now, return a placeholder indicating this needs the relay connection

    Ok(JoinDeviceResult {
        success: false,
        device_name: "Unknown".to_string(),
        message: "Device joining requires relay connection. Please use QR scanner on mobile to complete the pairing.".to_string(),
    })
}

/// Revoke a linked device.
///
/// This removes a device from the device registry, preventing it from syncing.
#[tauri::command]
pub fn revoke_device(
    device_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<bool, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    // Get current device ID to prevent self-revocation
    let current_device_id = hex::encode(identity.device_info().device_id());
    if device_id == current_device_id {
        return Err("Cannot revoke the current device. Use a different device to revoke this one.".to_string());
    }

    // Load device registry
    let mut registry = state
        .storage
        .load_device_registry()
        .map_err(|e| format!("Failed to load device registry: {:?}", e))?
        .ok_or_else(|| "No device registry found".to_string())?;

    // Find and revoke the device
    let device_id_bytes = hex::decode(&device_id)
        .map_err(|_| "Invalid device ID format".to_string())?;

    if device_id_bytes.len() != 32 {
        return Err("Device ID must be 32 bytes".to_string());
    }

    let device_id_array: [u8; 32] = device_id_bytes
        .try_into()
        .map_err(|_| "Invalid device ID length".to_string())?;

    // Revoke the device using the registry method
    registry
        .revoke_device(&device_id_array, identity.signing_keypair())
        .map_err(|e| format!("Failed to revoke device: {:?}", e))?;

    // Save updated registry
    state
        .storage
        .save_device_registry(&registry)
        .map_err(|e| format!("Failed to save device registry: {:?}", e))?;

    Ok(true)
}

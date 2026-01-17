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
    use webbook_core::exchange::DeviceLinkQR;
    let qr = DeviceLinkQR::generate(identity);

    Ok(qr.to_data_string())
}

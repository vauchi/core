//! Device Management Commands
//!
//! Commands for multi-device linking and management.

use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::State;
use vauchi_core::exchange::{DeviceLinkQR, DeviceLinkResponder, DeviceLinkResponse};
use vauchi_core::Identity;

use crate::state::AppState;

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "vauchi-local-storage";

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
    let mut state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    // Generate device link QR
    let qr = DeviceLinkQR::generate(identity);
    let qr_data = qr.to_data_string();

    // Store the QR data for use in complete_device_link
    state.pending_device_link_qr = Some(qr_data.clone());

    Ok(qr_data)
}

/// Result of starting a device join (step 1).
#[derive(Serialize)]
pub struct JoinStartResult {
    /// Whether the link data was valid.
    pub success: bool,
    /// The join request data to send to the existing device.
    pub request_data: Option<String>,
    /// Message for the user.
    pub message: String,
}

/// Result of finishing a device join (step 2).
#[derive(Serialize)]
pub struct JoinFinishResult {
    /// Whether the join completed successfully.
    pub success: bool,
    /// The display name from the joined identity.
    pub display_name: Option<String>,
    /// The device index assigned to this device.
    pub device_index: Option<u32>,
    /// Message for the user.
    pub message: String,
}

/// Pending device join state stored between steps.
#[derive(Serialize, Deserialize, Clone)]
struct PendingJoin {
    /// The original QR data string.
    qr_data: String,
    /// The device name for this new device.
    device_name: String,
}

/// Start joining another device using link data (Step 1).
///
/// This parses the QR data and creates a join request that must be sent
/// to the existing device. Store the response and call finish_join_device.
#[tauri::command]
pub fn join_device(
    link_data: String,
    device_name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<JoinStartResult, String> {
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

    // Use provided device name or default
    let device_name = if device_name.is_empty() {
        "Desktop Device".to_string()
    } else {
        device_name
    };

    // Create responder and generate request
    let responder = DeviceLinkResponder::from_qr(qr, device_name.clone())
        .map_err(|e| format!("Failed to create responder: {:?}", e))?;

    let encrypted_request = responder
        .create_request()
        .map_err(|e| format!("Failed to create request: {:?}", e))?;

    // Encode request for transmission
    let request_b64 = BASE64.encode(&encrypted_request);

    // Store pending join state
    let pending = PendingJoin {
        qr_data: link_data,
        device_name,
    };
    state.pending_device_join = Some(serde_json::to_string(&pending).unwrap_or_default());

    Ok(JoinStartResult {
        success: true,
        request_data: Some(request_b64),
        message: "Send this request to the existing device and get the response.".to_string(),
    })
}

/// Finish joining a device by processing the response (Step 2).
///
/// Call this after getting the response from the existing device.
#[tauri::command]
pub fn finish_join_device(
    response_data: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<JoinFinishResult, String> {
    let mut state = state.lock().unwrap();

    // Check if we already have an identity
    if state.identity.is_some() {
        return Err("This device already has an identity.".to_string());
    }

    // Get pending join state
    let pending_json = state
        .pending_device_join
        .take()
        .ok_or("No pending device join. Call join_device first.")?;

    let pending: PendingJoin = serde_json::from_str(&pending_json)
        .map_err(|_| "Invalid pending join state".to_string())?;

    // Parse the original QR data
    let qr = DeviceLinkQR::from_data_string(&pending.qr_data)
        .map_err(|e| format!("Invalid QR data: {:?}", e))?;

    // Decode the response
    let encrypted_response = BASE64
        .decode(&response_data)
        .map_err(|_| "Invalid response data (not valid base64)".to_string())?;

    // Decrypt the response using the link key
    let response = DeviceLinkResponse::decrypt(&encrypted_response, qr.link_key())
        .map_err(|e| format!("Failed to decrypt response: {:?}", e))?;

    // Create identity from the received seed
    let identity = Identity::from_device_link(
        *response.master_seed(),
        response.display_name().to_string(),
        response.device_index(),
        pending.device_name,
    );

    let display_name = identity.display_name().to_string();
    let device_index = identity.device_info().device_index();

    // Save identity backup to storage
    let backup = identity
        .export_backup(LOCAL_STORAGE_PASSWORD)
        .map_err(|e| format!("Failed to export backup: {:?}", e))?;

    state
        .storage
        .save_identity(backup.as_bytes(), &display_name)
        .map_err(|e| format!("Failed to save identity: {:?}", e))?;

    // Save device registry
    state
        .storage
        .save_device_registry(response.registry())
        .map_err(|e| format!("Failed to save device registry: {:?}", e))?;

    // Set identity in app state
    state.identity = Some(identity);

    Ok(JoinFinishResult {
        success: true,
        display_name: Some(display_name),
        device_index: Some(device_index),
        message: "Device successfully joined! Run sync to fetch contacts.".to_string(),
    })
}

/// Complete a device link request on the existing device.
///
/// This is called on the device that generated the link QR to approve the new device.
#[tauri::command]
pub fn complete_device_link(
    request_data: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or("No identity found. Cannot complete device link.")?;

    // Check for pending link QR
    let pending_qr_data = state
        .pending_device_link_qr
        .as_ref()
        .ok_or("No pending device link. Generate a link QR first.")?;

    let saved_qr = DeviceLinkQR::from_data_string(pending_qr_data)
        .map_err(|e| format!("Invalid saved QR data: {:?}", e))?;

    if saved_qr.is_expired() {
        return Err("Device link QR has expired. Generate a new one.".to_string());
    }

    // Get or create device registry
    let registry = state
        .storage
        .load_device_registry()
        .map_err(|e| format!("Failed to load registry: {:?}", e))?
        .unwrap_or_else(|| identity.initial_device_registry());

    // Restore the initiator
    let initiator = identity.restore_device_link_initiator(registry, saved_qr);

    // Decode and process the request
    let encrypted_request = BASE64
        .decode(&request_data)
        .map_err(|_| "Invalid request data (not valid base64)".to_string())?;

    let (encrypted_response, updated_registry, _new_device) = initiator
        .process_request(&encrypted_request)
        .map_err(|e| format!("Failed to process request: {:?}", e))?;

    // Save the updated registry
    state
        .storage
        .save_device_registry(&updated_registry)
        .map_err(|e| format!("Failed to save registry: {:?}", e))?;

    // Return the response for the new device
    Ok(BASE64.encode(&encrypted_response))
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

//! Exchange Commands

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Exchange QR data for the frontend.
#[derive(Serialize)]
pub struct ExchangeQR {
    pub data: String,
    pub display_name: String,
}

/// Generate QR code data for exchange.
#[tauri::command]
pub fn generate_qr(state: State<'_, Mutex<AppState>>) -> Result<ExchangeQR, String> {
    let state = state.lock().unwrap();

    let identity = state.identity.as_ref()
        .ok_or("No identity found")?;

    let public_id = identity.public_id();
    let display_name = identity.display_name().to_string();

    // Generate QR data (simplified format)
    let data = format!("wb://{}?name={}", public_id, display_name);

    Ok(ExchangeQR { data, display_name })
}

/// Complete an exchange with scanned data.
#[tauri::command]
pub fn complete_exchange(data: String, _state: State<'_, Mutex<AppState>>) -> Result<String, String> {
    // Parse the exchange data
    if !data.starts_with("wb://") {
        return Err("Invalid exchange data format".to_string());
    }

    // Extract public ID and name (simplified parsing)
    let rest = &data[5..];
    let parts: Vec<&str> = rest.split('?').collect();

    if parts.is_empty() {
        return Err("Invalid exchange data".to_string());
    }

    let _public_id = parts[0];
    let name = parts.get(1)
        .and_then(|p| p.strip_prefix("name="))
        .unwrap_or("Unknown");

    // TODO: Implement full X3DH exchange
    // For now, return the name as confirmation
    Ok(format!("Exchange initiated with: {}", name))
}

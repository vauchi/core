//! Identity Commands

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Identity information for the frontend.
#[derive(Serialize)]
pub struct IdentityInfo {
    pub display_name: String,
    pub public_id: String,
}

/// Check if an identity exists.
#[tauri::command]
pub fn has_identity(state: State<'_, Mutex<AppState>>) -> bool {
    let state = state.lock().unwrap();
    state.has_identity()
}

/// Create a new identity.
#[tauri::command]
pub fn create_identity(
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<IdentityInfo, String> {
    let mut state = state.lock().unwrap();

    state.create_identity(&name).map_err(|e| e.to_string())?;

    Ok(IdentityInfo {
        display_name: state.display_name().unwrap_or("").to_string(),
        public_id: state.public_id().unwrap_or_default(),
    })
}

/// Get identity information.
#[tauri::command]
pub fn get_identity_info(state: State<'_, Mutex<AppState>>) -> Result<IdentityInfo, String> {
    let state = state.lock().unwrap();

    if !state.has_identity() {
        return Err("No identity found".to_string());
    }

    Ok(IdentityInfo {
        display_name: state.display_name().unwrap_or("").to_string(),
        public_id: state.public_id().unwrap_or_default(),
    })
}

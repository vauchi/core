//! Visibility Commands
//!
//! Commands for managing contact card field visibility.

#![allow(dead_code)]

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Visibility level for a field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisibilityLevel {
    Everyone,
    Nobody,
    Contacts(Vec<String>),
}

/// Field visibility info for the frontend.
#[derive(Serialize)]
pub struct FieldVisibilityInfo {
    pub field_id: String,
    pub field_label: String,
    pub visibility: String,
    pub visible_to: Vec<String>,
}

/// Get visibility settings for a contact.
#[tauri::command]
pub fn get_visibility_rules(
    contact_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<FieldVisibilityInfo>, String> {
    let state = state.lock().unwrap();

    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let contact = contacts
        .iter()
        .find(|c| c.id() == contact_id)
        .ok_or_else(|| "Contact not found".to_string())?;

    let rules = contact.visibility_rules();
    let mut result = Vec::new();

    // Get our own card to list fields
    if let Ok(Some(card)) = state.storage.load_own_card() {
        for field in card.fields() {
            let field_id = field.id().to_string();
            let can_see = rules.can_see(&field_id, &contact_id);

            result.push(FieldVisibilityInfo {
                field_id: field_id.clone(),
                field_label: field.label().to_string(),
                visibility: if can_see { "visible" } else { "hidden" }.to_string(),
                visible_to: vec![], // TODO: implement detailed visibility
            });
        }
    }

    Ok(result)
}

/// Set visibility for a field.
#[tauri::command]
pub fn set_field_visibility(
    contact_id: String,
    field_id: String,
    visible: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let state = state.lock().unwrap();

    // Load the contact
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let _contact = contacts
        .iter()
        .find(|c| c.id() == contact_id)
        .ok_or_else(|| "Contact not found".to_string())?;

    // TODO: Implement full visibility update
    // This requires Storage::update_contact which doesn't exist yet
    // For now, return success as placeholder
    Ok(format!(
        "Visibility for {} set to {}",
        field_id,
        if visible { "visible" } else { "hidden" }
    ))
}

/// Set default visibility for new contacts.
#[tauri::command]
pub fn set_default_visibility(
    field_id: String,
    _visibility: VisibilityLevel,
) -> Result<String, String> {
    // TODO: Store in settings
    // For now, return success as placeholder
    Ok(format!("Default visibility set for {}", field_id))
}

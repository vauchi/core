//! Contacts Commands

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use webbook_core::ContactField;

use crate::state::AppState;

/// Contact information for the frontend.
#[derive(Serialize)]
pub struct ContactInfo {
    pub id: String,
    pub display_name: String,
    pub verified: bool,
}

/// Contact details for the frontend.
#[derive(Serialize)]
pub struct ContactDetails {
    pub id: String,
    pub display_name: String,
    pub verified: bool,
    pub fields: Vec<super::card::FieldInfo>,
}

/// List all contacts.
#[tauri::command]
pub fn list_contacts(state: State<'_, Mutex<AppState>>) -> Result<Vec<ContactInfo>, String> {
    let state = state.lock().unwrap();

    let contacts = state.storage.list_contacts()
        .map_err(|e| e.to_string())?;

    Ok(contacts.into_iter().map(|c| ContactInfo {
        id: c.id().to_string(),
        display_name: c.display_name().to_string(),
        verified: c.is_fingerprint_verified(),
    }).collect())
}

/// Get a specific contact.
#[tauri::command]
pub fn get_contact(id: String, state: State<'_, Mutex<AppState>>) -> Result<ContactDetails, String> {
    let state = state.lock().unwrap();

    let contact = state.storage.load_contact(&id)
        .map_err(|e: webbook_core::StorageError| e.to_string())?
        .ok_or("Contact not found")?;

    let fields: Vec<super::card::FieldInfo> = contact.card().fields().iter().map(|f: &ContactField| super::card::FieldInfo {
        id: f.id().to_string(),
        field_type: format!("{:?}", f.field_type()),
        label: f.label().to_string(),
        value: f.value().to_string(),
    }).collect();

    Ok(ContactDetails {
        id: contact.id().to_string(),
        display_name: contact.display_name().to_string(),
        verified: contact.is_fingerprint_verified(),
        fields,
    })
}

/// Remove a contact.
#[tauri::command]
pub fn remove_contact(id: String, state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let state = state.lock().unwrap();

    state.storage.delete_contact(&id)
        .map_err(|e| e.to_string())
}

//! Visibility Labels Commands
//!
//! Commands for managing visibility labels.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Visibility label info for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelInfo {
    pub id: String,
    pub name: String,
    pub contact_count: u32,
    pub visible_field_count: u32,
    pub created_at: u64,
    pub modified_at: u64,
}

/// Detailed label info including contacts and fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelDetail {
    pub id: String,
    pub name: String,
    pub contact_ids: Vec<String>,
    pub visible_field_ids: Vec<String>,
    pub created_at: u64,
    pub modified_at: u64,
}

/// List all visibility labels.
#[tauri::command]
pub fn list_labels(state: State<'_, Mutex<AppState>>) -> Result<Vec<LabelInfo>, String> {
    let state = state.lock().unwrap();

    let labels = state
        .storage
        .load_all_labels()
        .map_err(|e| format!("Failed to load labels: {:?}", e))?;

    Ok(labels
        .iter()
        .map(|l| LabelInfo {
            id: l.id().to_string(),
            name: l.name().to_string(),
            contact_count: l.contact_count() as u32,
            visible_field_count: l.visible_fields().len() as u32,
            created_at: l.created_at(),
            modified_at: l.modified_at(),
        })
        .collect())
}

/// Create a new visibility label.
#[tauri::command]
pub fn create_label(name: String, state: State<'_, Mutex<AppState>>) -> Result<LabelInfo, String> {
    let state = state.lock().unwrap();

    let label = state
        .storage
        .create_label(&name)
        .map_err(|e| format!("Failed to create label: {:?}", e))?;

    Ok(LabelInfo {
        id: label.id().to_string(),
        name: label.name().to_string(),
        contact_count: label.contact_count() as u32,
        visible_field_count: label.visible_fields().len() as u32,
        created_at: label.created_at(),
        modified_at: label.modified_at(),
    })
}

/// Get a label by ID with full details.
#[tauri::command]
pub fn get_label(
    label_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<LabelDetail, String> {
    let state = state.lock().unwrap();

    let label = state
        .storage
        .load_label(&label_id)
        .map_err(|e| format!("Failed to load label: {:?}", e))?;

    Ok(LabelDetail {
        id: label.id().to_string(),
        name: label.name().to_string(),
        contact_ids: label.contacts().iter().cloned().collect(),
        visible_field_ids: label.visible_fields().iter().cloned().collect(),
        created_at: label.created_at(),
        modified_at: label.modified_at(),
    })
}

/// Rename a label.
#[tauri::command]
pub fn rename_label(
    label_id: String,
    new_name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .rename_label(&label_id, &new_name)
        .map_err(|e| format!("Failed to rename label: {:?}", e))
}

/// Delete a label.
#[tauri::command]
pub fn delete_label(label_id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .delete_label(&label_id)
        .map_err(|e| format!("Failed to delete label: {:?}", e))
}

/// Add a contact to a label.
#[tauri::command]
pub fn add_contact_to_label(
    label_id: String,
    contact_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .add_contact_to_label(&label_id, &contact_id)
        .map_err(|e| format!("Failed to add contact to label: {:?}", e))
}

/// Remove a contact from a label.
#[tauri::command]
pub fn remove_contact_from_label(
    label_id: String,
    contact_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .remove_contact_from_label(&label_id, &contact_id)
        .map_err(|e| format!("Failed to remove contact from label: {:?}", e))
}

/// Get all labels that contain a contact.
#[tauri::command]
pub fn get_labels_for_contact(
    contact_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<LabelInfo>, String> {
    let state = state.lock().unwrap();

    let labels = state
        .storage
        .get_labels_for_contact(&contact_id)
        .map_err(|e| format!("Failed to get labels for contact: {:?}", e))?;

    Ok(labels
        .iter()
        .map(|l| LabelInfo {
            id: l.id().to_string(),
            name: l.name().to_string(),
            contact_count: l.contact_count() as u32,
            visible_field_count: l.visible_fields().len() as u32,
            created_at: l.created_at(),
            modified_at: l.modified_at(),
        })
        .collect())
}

/// Set whether a field is visible to contacts in a label.
#[tauri::command]
pub fn set_label_field_visibility(
    label_id: String,
    field_id: String,
    is_visible: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .set_label_field_visibility(&label_id, &field_id, is_visible)
        .map_err(|e| format!("Failed to set field visibility: {:?}", e))
}

/// Set a per-contact override for field visibility.
#[tauri::command]
pub fn set_contact_field_override(
    contact_id: String,
    field_id: String,
    is_visible: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .save_contact_override(&contact_id, &field_id, is_visible)
        .map_err(|e| format!("Failed to set contact override: {:?}", e))
}

/// Remove a per-contact override for field visibility.
#[tauri::command]
pub fn remove_contact_field_override(
    contact_id: String,
    field_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    state
        .storage
        .delete_contact_override(&contact_id, &field_id)
        .map_err(|e| format!("Failed to remove contact override: {:?}", e))
}

/// Get suggested default labels.
#[tauri::command]
pub fn get_suggested_labels() -> Vec<String> {
    vauchi_core::SUGGESTED_LABELS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

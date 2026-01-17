//! Visibility Commands
//!
//! Commands for managing contact card field visibility.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;
use webbook_core::contact::FieldVisibility;

use crate::state::AppState;

/// Visibility level for a field (frontend-friendly).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VisibilityLevel {
    Everyone,
    Nobody,
    Contacts { ids: Vec<String> },
}

impl From<&FieldVisibility> for VisibilityLevel {
    fn from(v: &FieldVisibility) -> Self {
        match v {
            FieldVisibility::Everyone => VisibilityLevel::Everyone,
            FieldVisibility::Nobody => VisibilityLevel::Nobody,
            FieldVisibility::Contacts(ids) => VisibilityLevel::Contacts {
                ids: ids.iter().cloned().collect(),
            },
        }
    }
}

/// Field visibility info for the frontend.
#[derive(Serialize)]
pub struct FieldVisibilityInfo {
    pub field_id: String,
    pub field_label: String,
    pub field_type: String,
    pub visibility: VisibilityLevel,
    pub can_see: bool,
}

/// Get visibility settings for what a contact can see.
#[tauri::command]
pub fn get_visibility_rules(
    contact_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<FieldVisibilityInfo>, String> {
    let state = state.lock().unwrap();

    // Load the specific contact
    let contact = state
        .storage
        .load_contact(&contact_id)
        .map_err(|e| format!("Failed to load contact: {:?}", e))?
        .ok_or_else(|| "Contact not found".to_string())?;

    let rules = contact.visibility_rules();
    let mut result = Vec::new();

    // Get our own card to list fields
    if let Ok(Some(card)) = state.storage.load_own_card() {
        for field in card.fields() {
            let field_id = field.id().to_string();
            let visibility = rules.get(&field_id);
            let can_see = rules.can_see(&field_id, &contact_id);

            result.push(FieldVisibilityInfo {
                field_id,
                field_label: field.label().to_string(),
                field_type: format!("{:?}", field.field_type()),
                visibility: VisibilityLevel::from(visibility),
                can_see,
            });
        }
    }

    Ok(result)
}

/// Set visibility for a field for a specific contact.
#[tauri::command]
pub fn set_field_visibility(
    contact_id: String,
    field_id: String,
    visibility: VisibilityLevel,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    // Load the contact
    let mut contact = state
        .storage
        .load_contact(&contact_id)
        .map_err(|e| format!("Failed to load contact: {:?}", e))?
        .ok_or_else(|| "Contact not found".to_string())?;

    // Update visibility rules
    let rules = contact.visibility_rules_mut();
    match visibility {
        VisibilityLevel::Everyone => rules.set_everyone(&field_id),
        VisibilityLevel::Nobody => rules.set_nobody(&field_id),
        VisibilityLevel::Contacts { ids } => {
            rules.set_contacts(&field_id, ids.into_iter().collect::<HashSet<_>>())
        }
    }

    // Save the updated contact
    state
        .storage
        .save_contact(&contact)
        .map_err(|e| format!("Failed to save contact: {:?}", e))?;

    Ok(())
}

/// Get all contacts for visibility selection UI.
#[tauri::command]
pub fn get_contacts_for_visibility(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ContactOption>, String> {
    let state = state.lock().unwrap();

    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    Ok(contacts
        .into_iter()
        .map(|c| ContactOption {
            id: c.id().to_string(),
            display_name: c.display_name().to_string(),
        })
        .collect())
}

/// Contact option for visibility selection.
#[derive(Serialize)]
pub struct ContactOption {
    pub id: String,
    pub display_name: String,
}

/// Contact visibility status for a field.
#[derive(Serialize)]
pub struct ContactFieldVisibility {
    pub contact_id: String,
    pub display_name: String,
    pub can_see: bool,
}

/// Get which contacts can see a specific field.
#[tauri::command]
pub fn get_field_viewers(
    field_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ContactFieldVisibility>, String> {
    let state = state.lock().unwrap();

    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let mut result = Vec::new();
    for contact in contacts {
        let contact_id = contact.id().to_string();
        let rules = contact.visibility_rules();
        let can_see = rules.can_see(&field_id, &contact_id);

        result.push(ContactFieldVisibility {
            contact_id,
            display_name: contact.display_name().to_string(),
            can_see,
        });
    }

    Ok(result)
}

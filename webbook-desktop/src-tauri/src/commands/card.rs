//! Card Commands

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use webbook_core::{ContactCard, ContactField, FieldType};

use crate::state::AppState;

/// Field information for the frontend.
#[derive(Serialize)]
pub struct FieldInfo {
    pub id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
}

/// Card information for the frontend.
#[derive(Serialize)]
pub struct CardInfo {
    pub display_name: String,
    pub fields: Vec<FieldInfo>,
}

/// Get the user's contact card.
#[tauri::command]
pub fn get_card(state: State<'_, Mutex<AppState>>) -> Result<CardInfo, String> {
    let state = state.lock().unwrap();

    let card = state.storage.load_own_card()
        .map_err(|e| e.to_string())?;

    match card {
        Some(c) => Ok(CardInfo {
            display_name: c.display_name().to_string(),
            fields: c.fields().iter().map(|f| FieldInfo {
                id: f.id().to_string(),
                field_type: format!("{:?}", f.field_type()),
                label: f.label().to_string(),
                value: f.value().to_string(),
            }).collect(),
        }),
        None => {
            // Return empty card with display name
            let display_name = state.display_name().unwrap_or("User");
            Ok(CardInfo {
                display_name: display_name.to_string(),
                fields: vec![],
            })
        }
    }
}

/// Add a field to the card.
#[tauri::command]
pub fn add_field(
    field_type: String,
    label: String,
    value: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let state = state.lock().unwrap();

    // Parse field type
    let ft = match field_type.to_lowercase().as_str() {
        "email" => FieldType::Email,
        "phone" => FieldType::Phone,
        "website" => FieldType::Website,
        "address" => FieldType::Address,
        "social" => FieldType::Social,
        _ => FieldType::Custom,
    };

    // Get or create card
    let mut card = state.storage.load_own_card()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| {
            ContactCard::new(state.display_name().unwrap_or("User"))
        });

    // Add field
    let field = ContactField::new(ft, &label, &value);
    card.add_field(field)
        .map_err(|e| format!("{}", e))?;

    // Save card
    state.storage.save_own_card(&card)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Remove a field from the card.
#[tauri::command]
pub fn remove_field(field_id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().unwrap();

    let mut card = state.storage.load_own_card()
        .map_err(|e| e.to_string())?
        .ok_or("No card found")?;

    card.remove_field(&field_id)
        .map_err(|e| format!("{}", e))?;

    state.storage.save_own_card(&card)
        .map_err(|e| e.to_string())?;

    Ok(())
}

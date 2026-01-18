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

    let contacts = state.storage.list_contacts().map_err(|e| e.to_string())?;

    Ok(contacts
        .into_iter()
        .map(|c| ContactInfo {
            id: c.id().to_string(),
            display_name: c.display_name().to_string(),
            verified: c.is_fingerprint_verified(),
        })
        .collect())
}

/// Get a specific contact.
#[tauri::command]
pub fn get_contact(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ContactDetails, String> {
    let state = state.lock().unwrap();

    let contact = state
        .storage
        .load_contact(&id)
        .map_err(|e: webbook_core::StorageError| e.to_string())?
        .ok_or("Contact not found")?;

    let fields: Vec<super::card::FieldInfo> = contact
        .card()
        .fields()
        .iter()
        .map(|f: &ContactField| super::card::FieldInfo {
            id: f.id().to_string(),
            field_type: format!("{:?}", f.field_type()),
            label: f.label().to_string(),
            value: f.value().to_string(),
        })
        .collect();

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

    state.storage.delete_contact(&id).map_err(|e| e.to_string())
}

/// Fingerprint info for verification.
#[derive(Serialize)]
pub struct FingerprintInfo {
    /// The contact's fingerprint as a hex string.
    pub their_fingerprint: String,
    /// Our fingerprint as a hex string.
    pub our_fingerprint: String,
    /// Human-readable fingerprint comparison (formatted for display).
    pub formatted_their: String,
    /// Human-readable fingerprint comparison (formatted for display).
    pub formatted_our: String,
}

/// Get fingerprint information for contact verification.
#[tauri::command]
pub fn get_contact_fingerprint(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<FingerprintInfo, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    let contact = state
        .storage
        .load_contact(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Contact not found")?;

    // Get raw public key bytes
    let their_pk_bytes = contact.public_key();
    let their_fingerprint = hex::encode(their_pk_bytes);

    let our_pk_bytes = identity.signing_keypair().public_key().as_bytes();
    let our_fingerprint = hex::encode(our_pk_bytes);

    // Format fingerprints for human comparison (groups of 4 chars)
    let format_fingerprint = |fp: &str| -> String {
        fp.chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase()
    };

    Ok(FingerprintInfo {
        their_fingerprint: their_fingerprint.clone(),
        our_fingerprint: our_fingerprint.clone(),
        formatted_their: format_fingerprint(&their_fingerprint),
        formatted_our: format_fingerprint(&our_fingerprint),
    })
}

/// Mark a contact as verified.
#[tauri::command]
pub fn verify_contact(id: String, state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let state = state.lock().unwrap();

    // Load the contact
    let mut contact = state
        .storage
        .load_contact(&id)
        .map_err(|e| e.to_string())?
        .ok_or("Contact not found")?;

    // Mark as verified
    contact.mark_fingerprint_verified();

    // Save the updated contact
    state
        .storage
        .save_contact(&contact)
        .map_err(|e| format!("Failed to save contact: {:?}", e))?;

    Ok(true)
}

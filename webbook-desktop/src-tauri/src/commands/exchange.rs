//! Exchange Commands
//!
//! Handles contact exchange via QR codes using X3DH key agreement.

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use webbook_core::exchange::{ExchangeQR as CoreExchangeQR, X3DH};
use webbook_core::{Contact, ContactCard};

use crate::state::AppState;

/// Exchange QR data for the frontend.
#[derive(Serialize)]
pub struct ExchangeQRResponse {
    /// Base64-encoded QR data string
    pub data: String,
    /// Display name shown in QR
    pub display_name: String,
    /// ASCII art representation of QR (for terminal/testing)
    pub qr_ascii: String,
}

/// Result of completing an exchange.
#[derive(Serialize)]
pub struct ExchangeResult {
    /// Whether the exchange was successful
    pub success: bool,
    /// The new contact's display name (placeholder until sync)
    pub contact_name: String,
    /// The new contact's public ID (hex-encoded)
    pub contact_id: String,
    /// Message for the user
    pub message: String,
}

/// Generate QR code data for exchange.
#[tauri::command]
pub fn generate_qr(state: State<'_, Mutex<AppState>>) -> Result<ExchangeQRResponse, String> {
    let state = state.lock().unwrap();

    let identity = state.identity.as_ref()
        .ok_or("No identity found. Please create an identity first.")?;

    // Generate proper ExchangeQR with X3DH keys
    let qr = CoreExchangeQR::generate(identity);

    let display_name = identity.display_name().to_string();
    let data = qr.to_data_string();
    let qr_ascii = qr.to_qr_image_string();

    Ok(ExchangeQRResponse {
        data,
        display_name,
        qr_ascii,
    })
}

/// Complete an exchange with scanned QR data.
///
/// Performs X3DH key agreement and creates a new contact.
#[tauri::command]
pub fn complete_exchange(data: String, state: State<'_, Mutex<AppState>>) -> Result<ExchangeResult, String> {
    let state = state.lock().unwrap();

    // Get our identity for X3DH key agreement
    let identity = state.identity.as_ref()
        .ok_or("No identity found. Please create an identity first.")?;

    // Parse the QR data (validates signature internally)
    let qr = CoreExchangeQR::from_data_string(&data)
        .map_err(|e| format!("Invalid QR code: {:?}", e))?;

    // Check if QR code has expired
    if qr.is_expired() {
        return Err("This QR code has expired. Please ask them to generate a new one.".to_string());
    }

    // Get their public key (identity) for contact ID
    let their_public_key = *qr.public_key();
    let their_exchange_key = *qr.exchange_key();
    let contact_id = hex::encode(their_public_key);

    // Check if we already have this contact
    if state.storage.load_contact(&contact_id).ok().flatten().is_some() {
        return Ok(ExchangeResult {
            success: false,
            contact_name: "Unknown".to_string(),
            contact_id,
            message: "You already have this contact.".to_string(),
        });
    }

    // Perform X3DH key agreement as initiator
    let our_x3dh = identity.x3dh_keypair();
    let (shared_secret, _ephemeral_public) = X3DH::initiate(&our_x3dh, &their_exchange_key)
        .map_err(|e| format!("Key agreement failed: {:?}", e))?;

    // Create a placeholder contact card
    // The real name will be received via relay sync
    let placeholder_name = format!("Contact {}", &contact_id[..8]);
    let card = ContactCard::new(&placeholder_name);

    // Create the contact with the shared secret
    let contact = Contact::from_exchange(their_public_key, card, shared_secret);

    // Save the contact to storage
    state.storage.save_contact(&contact)
        .map_err(|e| format!("Failed to save contact: {:?}", e))?;

    Ok(ExchangeResult {
        success: true,
        contact_name: placeholder_name,
        contact_id,
        message: "Contact added! Run sync to receive their contact card.".to_string(),
    })
}

//! Recovery Commands
//!
//! Commands for contact recovery via social vouching.

use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Serialize;
use tauri::State;
use webbook_core::recovery::{RecoveryClaim, RecoverySettings, RecoveryVoucher};

use crate::state::AppState;

/// Recovery status for the frontend.
#[derive(Serialize)]
#[allow(dead_code)]
pub struct RecoveryStatus {
    pub in_progress: bool,
    pub voucher_count: usize,
    pub threshold: u32,
    pub old_pk: Option<String>,
    pub new_pk: Option<String>,
}

/// Recovery settings for the frontend.
#[derive(Serialize)]
pub struct RecoverySettingsInfo {
    pub recovery_threshold: u32,
    pub verification_threshold: u32,
}

/// Voucher info for display.
#[derive(Serialize)]
#[allow(dead_code)]
pub struct VoucherInfo {
    pub voucher_pk: String,
    pub timestamp: u64,
}

/// Verification result for display.
#[derive(Serialize)]
#[allow(dead_code)]
pub struct VerificationInfo {
    pub confidence: String,
    pub mutual_vouchers: Vec<String>,
    pub total_vouchers: usize,
}

/// Get current recovery settings.
#[tauri::command]
pub fn get_recovery_settings() -> Result<RecoverySettingsInfo, String> {
    let settings = RecoverySettings::default();
    Ok(RecoverySettingsInfo {
        recovery_threshold: settings.recovery_threshold(),
        verification_threshold: settings.verification_threshold(),
    })
}

/// Create a recovery claim for a lost identity.
#[tauri::command]
pub fn create_recovery_claim(
    old_pk_hex: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    // Parse old public key
    let old_pk_bytes = hex::decode(&old_pk_hex).map_err(|e| format!("Invalid hex: {}", e))?;

    if old_pk_bytes.len() != 32 {
        return Err("Public key must be 32 bytes".to_string());
    }

    let mut old_pk = [0u8; 32];
    old_pk.copy_from_slice(&old_pk_bytes);

    let new_pk = identity.signing_public_key();

    // Sanity check
    if old_pk == *new_pk {
        return Err("Cannot create claim for your own current key".to_string());
    }

    let claim = RecoveryClaim::new(&old_pk, new_pk);
    let claim_b64 = BASE64.encode(claim.to_bytes());

    Ok(claim_b64)
}

/// Create a voucher for someone's recovery claim.
#[tauri::command]
pub fn create_recovery_voucher(
    claim_b64: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let state = state.lock().unwrap();

    let identity = state
        .identity
        .as_ref()
        .ok_or_else(|| "No identity found".to_string())?;

    // Parse claim
    let claim_bytes = BASE64
        .decode(&claim_b64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let claim =
        RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| format!("Invalid claim: {:?}", e))?;

    if claim.is_expired() {
        return Err("Claim has expired".to_string());
    }

    // Create voucher
    let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair())
        .map_err(|e| format!("Failed to create voucher: {:?}", e))?;

    let voucher_b64 = BASE64.encode(voucher.to_bytes());
    Ok(voucher_b64)
}

/// Check if a recovery claim matches a known contact.
#[tauri::command]
pub fn check_recovery_claim(
    claim_b64: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Option<String>, String> {
    let state = state.lock().unwrap();

    // Parse claim
    let claim_bytes = BASE64
        .decode(&claim_b64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let claim =
        RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| format!("Invalid claim: {:?}", e))?;

    let old_pk_hex = hex::encode(claim.old_pk());

    // Check if old_pk matches any contact
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    for contact in contacts {
        if hex::encode(contact.public_key()) == old_pk_hex {
            return Ok(Some(contact.display_name().to_string()));
        }
    }

    Ok(None)
}

/// Get recovery claim info without vouching.
#[derive(Serialize)]
pub struct ClaimInfo {
    pub old_pk: String,
    pub new_pk: String,
    pub is_expired: bool,
    pub contact_name: Option<String>,
}

#[tauri::command]
pub fn parse_recovery_claim(
    claim_b64: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ClaimInfo, String> {
    let state = state.lock().unwrap();

    let claim_bytes = BASE64
        .decode(&claim_b64)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    let claim =
        RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| format!("Invalid claim: {:?}", e))?;

    let old_pk_hex = hex::encode(claim.old_pk());
    let new_pk_hex = hex::encode(claim.new_pk());

    // Check if old_pk matches any contact
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let contact_name = contacts
        .iter()
        .find(|c| hex::encode(c.public_key()) == old_pk_hex)
        .map(|c| c.display_name().to_string());

    Ok(ClaimInfo {
        old_pk: old_pk_hex,
        new_pk: new_pk_hex,
        is_expired: claim.is_expired(),
        contact_name,
    })
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR Data Export
#![allow(dead_code)]
//!
//! Provides full data export for GDPR compliance (right to data portability).

use serde::Serialize;

use crate::storage::Storage;

/// Complete GDPR data export.
#[derive(Debug, Serialize)]
pub struct GdprExport {
    /// Export format version.
    pub version: u32,
    /// Export timestamp.
    pub exported_at: u64,
    /// Identity information (public data only, no raw keys).
    pub identity: Option<GdprIdentity>,
    /// All contacts (public data only).
    pub contacts: Vec<GdprContact>,
    /// Own contact card.
    pub own_card: Option<serde_json::Value>,
    /// Settings and preferences.
    pub settings: GdprSettings,
    /// Linked devices.
    pub devices: Option<Vec<GdprDevice>>,
    /// Recovery configuration.
    pub recovery_config: Option<GdprRecoveryConfig>,
}

/// Identity data for GDPR export (no raw keys).
#[derive(Debug, Serialize)]
pub struct GdprIdentity {
    pub display_name: String,
    pub public_id: String,
    pub created_at: u64,
}

/// Contact data for GDPR export.
#[derive(Debug, Serialize)]
pub struct GdprContact {
    pub display_name: String,
    pub public_key_fingerprint: String,
    pub exchange_timestamp: u64,
    pub fingerprint_verified: bool,
    pub card_fields: Vec<GdprField>,
}

/// Field data for GDPR export.
#[derive(Debug, Serialize)]
pub struct GdprField {
    pub field_type: String,
    pub label: String,
    pub value: String,
}

/// Settings data for GDPR export.
#[derive(Debug, Serialize)]
pub struct GdprSettings {
    pub consent_records: Vec<serde_json::Value>,
}

/// Device data for GDPR export.
#[derive(Debug, Serialize)]
pub struct GdprDevice {
    pub device_name: String,
    pub device_index: u32,
    pub created_at: u64,
}

/// Recovery configuration for GDPR export.
#[derive(Debug, Serialize)]
pub struct GdprRecoveryConfig {
    pub trusted_contacts_count: usize,
    pub threshold: Option<u32>,
}

/// Exports all user data for GDPR compliance.
///
/// Returns a structured export containing all personal data stored locally.
/// Raw cryptographic keys are excluded — only public identifiers are included.
pub fn export_all_data(storage: &Storage) -> Result<GdprExport, crate::storage::StorageError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Export contacts
    let contacts = storage.list_contacts()?;
    let gdpr_contacts: Vec<GdprContact> = contacts
        .iter()
        .map(|c| {
            let fields: Vec<GdprField> = c
                .card()
                .fields()
                .iter()
                .map(|f| GdprField {
                    field_type: format!("{:?}", f.field_type()),
                    label: f.label().to_string(),
                    value: f.value().to_string(),
                })
                .collect();

            GdprContact {
                display_name: c.display_name().to_string(),
                public_key_fingerprint: c.fingerprint(),
                exchange_timestamp: c.exchange_timestamp(),
                fingerprint_verified: c.is_fingerprint_verified(),
                card_fields: fields,
            }
        })
        .collect();

    // Export own card
    let own_card = storage
        .load_own_card()?
        .map(|card| serde_json::to_value(&card).unwrap_or(serde_json::Value::Null));

    // Export consent records
    let consent_records = storage
        .list_consent_records_with_version()?
        .into_iter()
        .map(|(id, ct, granted, ts, pv)| {
            serde_json::json!({
                "id": id,
                "consent_type": ct,
                "granted": granted,
                "timestamp": ts,
                "policy_version": pv,
            })
        })
        .collect();

    // Export devices (best effort — device info may not be configured)
    let devices = export_devices(storage);

    // Export recovery config (count of trusted contacts)
    let trusted_count = contacts.iter().filter(|c| c.is_recovery_trusted()).count();
    let recovery_config = Some(GdprRecoveryConfig {
        trusted_contacts_count: trusted_count,
        threshold: None, // Set by caller who has RecoverySettings access
    });

    Ok(GdprExport {
        version: 2,
        exported_at: now,
        identity: None, // Set by caller who has Identity access
        contacts: gdpr_contacts,
        own_card,
        settings: GdprSettings { consent_records },
        devices,
        recovery_config,
    })
}

/// Exports device information (best effort).
fn export_devices(storage: &Storage) -> Option<Vec<GdprDevice>> {
    // Try to load device registry from storage
    // Returns empty list if no devices registered
    match storage.load_device_registry_json() {
        Ok(Some(json)) => {
            // Parse device registry JSON
            if let Ok(registry) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(devices) = registry.get("devices").and_then(|d| d.as_array()) {
                    let gdpr_devices: Vec<GdprDevice> = devices
                        .iter()
                        .map(|d| GdprDevice {
                            device_name: d
                                .get("device_name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("Unknown")
                                .to_string(),
                            device_index: d
                                .get("device_index")
                                .and_then(|i| i.as_u64())
                                .unwrap_or(0) as u32,
                            created_at: d.get("created_at").and_then(|t| t.as_u64()).unwrap_or(0),
                        })
                        .collect();
                    return Some(gdpr_devices);
                }
            }
            Some(Vec::new())
        }
        _ => Some(Vec::new()),
    }
}

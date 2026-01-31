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

    Ok(GdprExport {
        version: 1,
        exported_at: now,
        identity: None, // Set by caller who has Identity access
        contacts: gdpr_contacts,
        own_card,
        settings: GdprSettings {
            consent_records: Vec::new(),
        },
    })
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR Data Export
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
    /// Audit log entries.
    pub audit_log: Vec<serde_json::Value>,
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

    // Export audit log (Art 15 — access to all personal data)
    // Filter sensitive key material from details before export (#21)
    let audit_log = storage
        .list_audit_log()?
        .into_iter()
        .map(|(event_type, details, timestamp)| {
            let filtered_details = details.map(|d| filter_audit_details(&d));
            serde_json::json!({
                "event_type": event_type,
                "details": filtered_details,
                "timestamp": timestamp,
            })
        })
        .collect();

    // Log the export event itself
    storage.log_audit_event("gdpr_data_export", None)?;

    Ok(GdprExport {
        version: 3,
        exported_at: now,
        identity: None, // Set by caller who has Identity access
        contacts: gdpr_contacts,
        own_card,
        settings: GdprSettings { consent_records },
        devices,
        recovery_config,
        audit_log,
    })
}

/// Strips sensitive cryptographic fields from audit log details (#21).
///
/// If the details string is valid JSON, removes fields whose keys contain
/// sensitive terms (key, nonce, secret, seed, etc.). Otherwise, redacts
/// long hex strings that likely represent key material.
fn filter_audit_details(details: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(details) {
        if let Some(obj) = value.as_object_mut() {
            let sensitive = [
                "key",
                "nonce",
                "secret",
                "seed",
                "private_key",
                "encryption_key",
                "cek",
                "smk",
                "sek",
                "fkek",
                "ratchet_state",
                "shared_secret",
            ];
            obj.retain(|k, _| {
                let lower = k.to_lowercase();
                !sensitive.iter().any(|s| lower.contains(s))
            });
        }
        serde_json::to_string(&value).unwrap_or_else(|_| details.to_string())
    } else {
        redact_hex_strings(details)
    }
}

/// Replaces hex strings of 32+ characters with `[redacted]`.
fn redact_hex_strings(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut hex_run = 0usize;
    let mut hex_start = 0usize;

    for (i, ch) in text.char_indices() {
        if ch.is_ascii_hexdigit() {
            if hex_run == 0 {
                hex_start = i;
            }
            hex_run += 1;
        } else {
            if hex_run >= 32 {
                result.push_str("[redacted]");
            } else {
                result.push_str(&text[hex_start..hex_start + hex_run]);
            }
            result.push(ch);
            hex_run = 0;
        }
    }
    // Handle trailing hex run
    if hex_run >= 32 {
        result.push_str("[redacted]");
    } else if hex_run > 0 {
        result.push_str(&text[text.len() - hex_run..]);
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_audit_details_strips_json_key_fields() {
        let input = r#"{"action":"exchange","encryption_key":"deadbeef","contact":"Bob"}"#;
        let result = filter_audit_details(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("contact").is_some());
        assert!(parsed.get("action").is_some());
        assert!(parsed.get("encryption_key").is_none());
    }

    #[test]
    fn test_filter_audit_details_strips_nonce_and_secret() {
        let input = r#"{"nonce":"abc","shared_secret":"xyz","event":"sync"}"#;
        let result = filter_audit_details(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("event").is_some());
        assert!(parsed.get("nonce").is_none());
        assert!(parsed.get("shared_secret").is_none());
    }

    #[test]
    fn test_filter_audit_details_strips_cek_smk_sek() {
        let input = r#"{"cek":"abc","smk":"def","sek":"ghi","contact_id":"123"}"#;
        let result = filter_audit_details(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("contact_id").is_some());
        assert!(parsed.get("cek").is_none());
        assert!(parsed.get("smk").is_none());
        assert!(parsed.get("sek").is_none());
    }

    #[test]
    fn test_filter_audit_details_case_insensitive() {
        let input = r#"{"Encryption_Key":"val","Private_Key":"val2","ok":"yes"}"#;
        let result = filter_audit_details(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("ok").is_some());
        assert!(parsed.get("Encryption_Key").is_none());
        assert!(parsed.get("Private_Key").is_none());
    }

    #[test]
    fn test_filter_audit_details_preserves_non_sensitive_json() {
        let input = r#"{"action":"delete","contact_id":"abc-123","timestamp":1234}"#;
        let result = filter_audit_details(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_redact_hex_strings_redacts_long_hex() {
        let input = "prefix_aabbccdd00112233445566778899aabbccdd00112233_suffix";
        let result = redact_hex_strings(input);
        assert!(result.contains("[redacted]"));
        assert!(result.contains("prefix_"));
        assert!(result.contains("_suffix"));
    }

    #[test]
    fn test_redact_hex_strings_preserves_short_hex() {
        let input = "id=abcd1234 done";
        let result = redact_hex_strings(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_filter_audit_details_non_json_with_hex() {
        let hex64 = "a".repeat(64);
        let input = format!("key={hex64} ok");
        let result = filter_audit_details(&input);
        assert!(result.contains("[redacted]"));
        assert!(!result.contains(&hex64));
    }

    #[test]
    fn test_filter_audit_details_empty_json() {
        let result = filter_audit_details("{}");
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_filter_audit_details_plain_text() {
        let result = filter_audit_details("just a plain log message");
        assert_eq!(result, "just a plain log message");
    }

    #[test]
    fn test_filter_strips_ratchet_and_seed() {
        let input = r#"{"ratchet_state":"abc","seed":"xyz","count":5}"#;
        let result = filter_audit_details(input);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("count").is_some());
        assert!(parsed.get("ratchet_state").is_none());
        assert!(parsed.get("seed").is_none());
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consent Management
//!
//! Tracks user consent for data processing activities (GDPR Article 7).

use serde::{Deserialize, Serialize};

/// Types of consent that can be granted or revoked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConsentType {
    /// Consent for local data processing (required for operation).
    DataProcessing,
    /// Consent for sharing contact information with exchanged contacts.
    ContactSharing,
    /// Consent to participate in recovery vouching.
    RecoveryVouching,
}

impl ConsentType {
    fn as_str(&self) -> &'static str {
        match self {
            ConsentType::DataProcessing => "data_processing",
            ConsentType::ContactSharing => "contact_sharing",
            ConsentType::RecoveryVouching => "recovery_vouching",
        }
    }

    /// Parses a consent type from its string representation.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "data_processing" => Some(ConsentType::DataProcessing),
            "contact_sharing" => Some(ConsentType::ContactSharing),
            "recovery_vouching" => Some(ConsentType::RecoveryVouching),
            _ => None,
        }
    }
}

/// A recorded consent decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    /// Unique record ID.
    pub id: String,
    /// Type of consent.
    pub consent_type: ConsentType,
    /// Whether consent was granted (true) or revoked (false).
    pub granted: bool,
    /// Unix timestamp of the decision.
    pub timestamp: u64,
    /// Privacy policy version at time of consent.
    #[serde(default)]
    pub policy_version: Option<String>,
}

/// Aggregated consent status for a specific consent type.
///
/// Combines the boolean grant status with metadata from the consent log
/// (timestamp of last change, policy version). Eliminates the need for
/// clients to query multiple APIs and assemble this information inline.
#[derive(Debug, Clone)]
pub struct ConsentStatus {
    /// Whether consent is currently granted.
    pub granted: bool,
    /// Unix timestamp of the most recent grant or revocation, if any.
    pub last_changed_at: Option<u64>,
    /// Privacy policy version from the most recent consent record, if any.
    pub policy_version: Option<String>,
}

/// Manages consent records in storage.
pub struct ConsentManager<'a> {
    storage: &'a crate::storage::Storage,
}

impl<'a> ConsentManager<'a> {
    /// Creates a new ConsentManager.
    pub fn new(storage: &'a crate::storage::Storage) -> Self {
        ConsentManager { storage }
    }

    /// Grants consent for a specific type.
    pub fn grant(&self, consent_type: ConsentType) -> Result<(), crate::storage::StorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = crate::clock::ambient_now_secs();

        self.storage
            .execute_consent_upsert(&id, consent_type.as_str(), true, now)
    }

    /// Revokes consent for a specific type.
    pub fn revoke(&self, consent_type: ConsentType) -> Result<(), crate::storage::StorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = crate::clock::ambient_now_secs();

        self.storage
            .execute_consent_upsert(&id, consent_type.as_str(), false, now)
    }

    /// Checks whether consent is currently granted for a type.
    pub fn check(&self, consent_type: &ConsentType) -> Result<bool, crate::storage::StorageError> {
        self.storage.check_consent(consent_type.as_str())
    }

    /// Grants consent with a specific policy version.
    pub fn grant_with_version(
        &self,
        consent_type: ConsentType,
        policy_version: &str,
    ) -> Result<(), crate::storage::StorageError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = crate::clock::ambient_now_secs();

        self.storage.execute_consent_upsert_with_version(
            &id,
            consent_type.as_str(),
            true,
            now,
            policy_version,
        )
    }

    /// Exports all consent records.
    pub fn export_consent_log(&self) -> Result<Vec<ConsentRecord>, crate::storage::StorageError> {
        let rows = self.storage.list_consent_records()?;
        let records = rows
            .into_iter()
            .filter_map(|(id, ct_str, granted, ts)| {
                ConsentType::parse(&ct_str).map(|ct| ConsentRecord {
                    id,
                    consent_type: ct,
                    granted,
                    timestamp: ts,
                    policy_version: None,
                })
            })
            .collect();
        Ok(records)
    }

    /// Exports all consent records including policy version.
    pub fn export_consent_log_with_version(
        &self,
    ) -> Result<Vec<ConsentRecord>, crate::storage::StorageError> {
        let rows = self.storage.list_consent_records_with_version()?;
        let records = rows
            .into_iter()
            .filter_map(|(id, ct_str, granted, ts, pv)| {
                ConsentType::parse(&ct_str).map(|ct| ConsentRecord {
                    id,
                    consent_type: ct,
                    granted,
                    timestamp: ts,
                    policy_version: pv,
                })
            })
            .collect();
        Ok(records)
    }
}

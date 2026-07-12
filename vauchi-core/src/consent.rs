// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consent domain types.
//!
//! Consent decisions (GDPR Article 7) recorded and queried by the
//! `ConsentManager` in `api::consent`. The types live here — always
//! compiled — rather than beside the manager, because `api` is gated
//! behind the `network-rustls` feature; keeping them ungated means the
//! crate-root re-export (`vauchi_core::ConsentType`) never disappears
//! with the feature.

/// Types of consent that can be granted or revoked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    // Only called by the api-gated ConsentManager; compiled in all
    // configs so a future ungated caller needs no feature surgery.
    #[cfg_attr(not(feature = "network-rustls"), allow(dead_code))]
    pub(crate) fn as_str(&self) -> &'static str {
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

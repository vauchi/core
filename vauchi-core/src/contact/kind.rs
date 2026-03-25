// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact kind types: Exchanged (crypto) vs Imported (no crypto).
//!
//! HR-1: Crypto fields live ONLY in [`ExchangedData`]. This makes it structurally
//! impossible for imported contacts to have trust flags, keys, or relay channels.

use serde::{Deserialize, Serialize};

use crate::contact::VisibilityRules;
use crate::crypto::SymmetricKey;
use crate::exchange::{ExchangeTransport, ProximityConfidence, TrustMetrics};

/// Distinguishes exchanged contacts (with crypto) from imported contacts (no crypto).
#[derive(Clone, Debug)]
pub enum ContactKind {
    /// A contact obtained through in-person cryptographic exchange.
    Exchanged(ExchangedData),
    /// A contact imported from an external source (vCard, CSV, platform).
    Imported(ImportedData),
}

impl ContactKind {
    /// Returns `true` if this is an exchanged (crypto) contact.
    pub fn is_exchanged(&self) -> bool {
        matches!(self, ContactKind::Exchanged(_))
    }

    /// Returns `true` if this is an imported (non-crypto) contact.
    pub fn is_imported(&self) -> bool {
        matches!(self, ContactKind::Imported(_))
    }

    /// Returns a reference to the exchanged data, if this is an exchanged contact.
    pub fn exchanged_data(&self) -> Option<&ExchangedData> {
        match self {
            ContactKind::Exchanged(data) => Some(data),
            ContactKind::Imported(_) => None,
        }
    }

    /// Returns a mutable reference to the exchanged data, if this is an exchanged contact.
    pub fn exchanged_data_mut(&mut self) -> Option<&mut ExchangedData> {
        match self {
            ContactKind::Exchanged(data) => Some(data),
            ContactKind::Imported(_) => None,
        }
    }

    /// Returns a reference to the imported data, if this is an imported contact.
    pub fn imported_data(&self) -> Option<&ImportedData> {
        match self {
            ContactKind::Imported(data) => Some(data),
            ContactKind::Exchanged(_) => None,
        }
    }
}

/// Crypto and trust fields for an exchanged contact.
///
/// These fields will later move out of `Contact` into this struct,
/// ensuring imported contacts cannot structurally hold crypto state.
#[derive(Clone, Debug)]
pub struct ExchangedData {
    /// Their Ed25519 public key.
    pub public_key: [u8; 32],
    /// Shared symmetric key for communication.
    pub shared_key: SymmetricKey,
    /// Unix timestamp of when the exchange occurred.
    pub exchange_timestamp: u64,
    /// How this contact was established (QR, NFC, BLE, etc.).
    pub exchange_transport: ExchangeTransport,
    /// Whether the user manually verified their fingerprint.
    pub fingerprint_verified: bool,
    /// Whether this contact is trusted for recovery purposes.
    pub recovery_trusted: bool,
    /// Whether this contact is trusted for simplified contact proposals.
    pub proposal_trusted: bool,
    /// Proximity confidence level from the exchange.
    pub proximity_confidence: ProximityConfidence,
    /// Whether this contact has undergone identity recovery.
    pub has_recovered: bool,
    /// Relay URL learned during exchange (for per-contact relay routing).
    pub relay_url: Option<String>,
    /// Relay's Noise NK public key, pinned during in-person exchange.
    pub relay_noise_pubkey: Option<[u8; 32]>,
    /// Full trust metrics from the exchange. None for legacy contacts.
    pub trust_metrics: Option<TrustMetrics>,
    /// Our visibility rules for this contact (what they can see of our card).
    pub visibility_rules: VisibilityRules,
}

/// Metadata for an imported (non-crypto) contact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedData {
    /// Where this contact was imported from.
    pub source: ImportSource,
    /// Unix timestamp of when the import occurred.
    pub imported_at: u64,
    /// Original UID from the source (e.g., vCard UID, platform contact ID).
    pub original_uid: Option<String>,
}

/// The source from which a contact was imported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    /// Imported from a vCard (.vcf) file.
    VcardFile,
    /// Imported from a CSV file.
    CsvFile,
    /// Imported from iOS Contacts (CNContact).
    IosPlatform,
    /// Imported from Android Contacts (ContactsContract).
    AndroidPlatform,
    /// Manually created by the user.
    Manual,
}

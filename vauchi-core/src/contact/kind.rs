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
use crate::exchange::TrustMetrics;
use crate::types::{ExchangeTransport, ProximityConfidence};

/// Distinguishes exchanged contacts (with crypto) from imported contacts (no crypto).
#[derive(Clone, Debug)]
#[non_exhaustive]
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
    pub(crate) public_key: [u8; 32],
    /// Shared symmetric key for communication.
    pub(crate) shared_key: SymmetricKey,
    /// Unix timestamp of when the exchange occurred.
    pub(crate) exchange_timestamp: u64,
    /// How this contact was established (QR, NFC, BLE, etc.).
    pub(crate) exchange_transport: ExchangeTransport,
    /// Whether the user manually verified their fingerprint.
    pub(crate) fingerprint_verified: bool,
    /// Whether this contact is trusted for recovery purposes.
    pub(crate) recovery_trusted: bool,
    /// Whether this contact is trusted for simplified contact proposals.
    pub(crate) proposal_trusted: bool,
    /// Proximity confidence level from the exchange.
    pub(crate) proximity_confidence: ProximityConfidence,
    /// Whether this contact has undergone identity recovery.
    pub(crate) has_recovered: bool,
    /// Relay URL learned during exchange (for per-contact relay routing).
    pub(crate) relay_url: Option<String>,
    /// Relay's Noise NK public key, pinned during in-person exchange.
    pub(crate) relay_noise_pubkey: Option<[u8; 32]>,
    /// Full trust metrics from the exchange. None for legacy contacts.
    pub(crate) trust_metrics: Option<TrustMetrics>,
    /// Our visibility rules for this contact (what they can see of our card).
    pub(crate) visibility_rules: VisibilityRules,
}

#[cfg(any(test, feature = "testing"))]
impl ExchangedData {
    /// Creates an `ExchangedData` with all fields specified. Only available in tests.
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_test(
        public_key: [u8; 32],
        shared_key: SymmetricKey,
        exchange_timestamp: u64,
        exchange_transport: ExchangeTransport,
        fingerprint_verified: bool,
        recovery_trusted: bool,
        proposal_trusted: bool,
        proximity_confidence: ProximityConfidence,
        has_recovered: bool,
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
        trust_metrics: Option<TrustMetrics>,
        visibility_rules: VisibilityRules,
    ) -> Self {
        Self {
            public_key,
            shared_key,
            exchange_timestamp,
            exchange_transport,
            fingerprint_verified,
            recovery_trusted,
            proposal_trusted,
            proximity_confidence,
            has_recovered,
            relay_url,
            relay_noise_pubkey,
            trust_metrics,
            visibility_rules,
        }
    }

    /// Returns the public key. Only available in tests.
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    /// Returns whether the fingerprint was verified. Only available in tests.
    pub fn fingerprint_verified(&self) -> bool {
        self.fingerprint_verified
    }

    /// Sets the fingerprint verified flag. Only available in tests.
    pub fn set_fingerprint_verified(&mut self, val: bool) {
        self.fingerprint_verified = val;
    }
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
#[non_exhaustive]
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

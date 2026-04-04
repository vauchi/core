// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core contact and exchange types.
//!
//! Field types, contact fields, contact cards, contacts, and exchange results.

use vauchi_core::contact::trust::TrustLevel;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType};

/// Mobile-friendly contact trust level derived from cryptographic exchange facts.
///
/// This is distinct from `MobileTrustLevel` (which reflects social validation counts).
/// `MobileContactTrustLevel` is computed deterministically from exchange metadata —
/// it is never user-editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileContactTrustLevel {
    /// Identity was recovered — ratchet may have reset. Highest caution.
    Cautious,
    /// User manually verified the key fingerprint out-of-band.
    Verified,
    /// High proximity confidence with a close-range transport (NFC or BLE).
    High,
    /// Normal exchange, no special indicators. Default.
    Standard,
}

impl From<TrustLevel> for MobileContactTrustLevel {
    fn from(t: TrustLevel) -> Self {
        match t {
            TrustLevel::Cautious => MobileContactTrustLevel::Cautious,
            TrustLevel::Verified => MobileContactTrustLevel::Verified,
            TrustLevel::High => MobileContactTrustLevel::High,
            TrustLevel::Standard => MobileContactTrustLevel::Standard,
            _ => MobileContactTrustLevel::Standard,
        }
    }
}

/// Mobile-friendly field type enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileFieldType {
    Email,
    Phone,
    Website,
    Address,
    Social,
    Birthday,
    Custom,
}

impl From<FieldType> for MobileFieldType {
    fn from(ft: FieldType) -> Self {
        match ft {
            FieldType::Email => MobileFieldType::Email,
            FieldType::Phone => MobileFieldType::Phone,
            FieldType::Website => MobileFieldType::Website,
            FieldType::Address => MobileFieldType::Address,
            FieldType::Social => MobileFieldType::Social,
            FieldType::Birthday => MobileFieldType::Birthday,
            FieldType::Custom => MobileFieldType::Custom,
            _ => MobileFieldType::Custom,
        }
    }
}

impl From<MobileFieldType> for FieldType {
    fn from(mft: MobileFieldType) -> Self {
        match mft {
            MobileFieldType::Email => FieldType::Email,
            MobileFieldType::Phone => FieldType::Phone,
            MobileFieldType::Website => FieldType::Website,
            MobileFieldType::Address => FieldType::Address,
            MobileFieldType::Social => FieldType::Social,
            MobileFieldType::Birthday => FieldType::Birthday,
            MobileFieldType::Custom => FieldType::Custom,
        }
    }
}

/// Mobile-friendly contact field.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContactField {
    pub id: String,
    pub field_type: MobileFieldType,
    pub label: String,
    pub value: String,
    /// Private per-field annotation (your eyes only — never sent to other contacts).
    pub note: Option<String>,
}

impl From<&ContactField> for MobileContactField {
    fn from(field: &ContactField) -> Self {
        MobileContactField {
            id: field.id().to_string(),
            field_type: field.field_type().into(),
            label: field.label().to_string(),
            value: field.value().to_string(),
            note: field.note().map(|s| s.to_string()),
        }
    }
}

/// Mobile-friendly contact card.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContactCard {
    pub display_name: String,
    pub fields: Vec<MobileContactField>,
}

impl From<&ContactCard> for MobileContactCard {
    fn from(card: &ContactCard) -> Self {
        MobileContactCard {
            display_name: card.display_name().to_string(),
            fields: card.fields().iter().map(MobileContactField::from).collect(),
        }
    }
}

/// Mobile-friendly contact.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContact {
    pub id: String,
    pub display_name: String,
    pub fingerprint: String,
    pub is_verified: bool,
    pub is_recovery_trusted: bool,
    pub is_hidden: bool,
    pub card: MobileContactCard,
    pub added_at: u64,
    /// Cryptographic trust level derived from exchange facts.
    pub trust_level: MobileContactTrustLevel,
    /// Transport used during the original exchange (e.g. "qr", "nfc", "ble").
    pub exchange_transport: String,
    /// Proximity confidence from the original exchange (e.g. "high", "medium", "low", "unknown").
    pub proximity_confidence: String,
    /// Whether this contact is trusted for simplified contact proposals (local-only flag).
    pub proposal_trusted: bool,
    /// Transport proximity level from the original exchange (e.g. "physical", "contact_range", "proximate", "none", "unknown").
    pub transport_proximity: String,
    /// Whether this contact has trust metrics recorded from a full exchange session.
    pub has_trust_metrics: bool,
    /// Exchange reciprocity status (orthogonal to trust level).
    pub reciprocity: MobileReciprocity,
}

/// Exchange reciprocity status — whether the other party also completed the exchange.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileReciprocity {
    Confirmed,
    Pending,
    Unreciprocated,
    Unknown,
}

impl From<&Contact> for MobileContact {
    fn from(contact: &Contact) -> Self {
        use vauchi_core::types::{ExchangeTransport, ProximityConfidence};

        let exchange_transport = match contact.exchange_transport() {
            Some(ExchangeTransport::Qr) | None => "qr",
            Some(ExchangeTransport::Nfc) => "nfc",
            Some(ExchangeTransport::Ble) => "ble",
            Some(ExchangeTransport::Usb) => "usb",
            Some(ExchangeTransport::Audio) => "audio",
            _ => "qr",
        }
        .to_string();

        let proximity_confidence = match contact.proximity_confidence() {
            ProximityConfidence::High => "high",
            ProximityConfidence::Medium => "medium",
            ProximityConfidence::Low => "low",
            ProximityConfidence::Unknown => "unknown",
            _ => "unknown",
        }
        .to_string();

        let (transport_proximity, has_trust_metrics) = match contact.trust_metrics() {
            Some(m) => {
                use vauchi_core::exchange::TransportProximity;
                let prox = match m.transport_proximity {
                    TransportProximity::Physical => "physical",
                    TransportProximity::ContactRange => "contact_range",
                    TransportProximity::Proximate => "proximate",
                    TransportProximity::None => "none",
                    _ => "unknown",
                };
                (prox.to_string(), true)
            }
            None => ("unknown".to_string(), false),
        };

        MobileContact {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            fingerprint: contact.fingerprint(),
            is_verified: contact.is_fingerprint_verified(),
            is_recovery_trusted: contact.is_recovery_trusted(),
            is_hidden: contact.is_hidden(),
            card: MobileContactCard::from(contact.card()),
            added_at: contact.exchange_timestamp().unwrap_or(0),
            trust_level: contact.trust_level().into(),
            exchange_transport,
            proximity_confidence,
            proposal_trusted: contact.is_proposal_trusted(),
            transport_proximity,
            has_trust_metrics,
            reciprocity: {
                use vauchi_core::exchange::reciprocity::Reciprocity;
                match contact.reciprocity() {
                    Reciprocity::Confirmed => MobileReciprocity::Confirmed,
                    Reciprocity::Pending => MobileReciprocity::Pending,
                    Reciprocity::Unreciprocated => MobileReciprocity::Unreciprocated,
                    _ => MobileReciprocity::Unknown,
                }
            },
        }
    }
}

/// A per-field private note entry (field_id + note text).
///
/// Used as a Vec-based alternative to HashMap for UniFFI compatibility.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileFieldNote {
    pub field_id: String,
    pub note: String,
}

/// Exchange result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileExchangeResult {
    pub contact_id: String,
    pub contact_name: String,
    pub success: bool,
    pub error_message: Option<String>,
}

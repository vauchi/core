//! Mobile-friendly data types.
//!
//! These types are wrappers around webbook-core types that are compatible
//! with UniFFI for cross-language bindings.

use webbook_core::{Contact, ContactCard, ContactField, FieldType};

/// Mobile-friendly field type enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileFieldType {
    Email,
    Phone,
    Website,
    Address,
    Social,
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
            FieldType::Custom => MobileFieldType::Custom,
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
}

impl From<&ContactField> for MobileContactField {
    fn from(field: &ContactField) -> Self {
        MobileContactField {
            id: field.id().to_string(),
            field_type: field.field_type().into(),
            label: field.label().to_string(),
            value: field.value().to_string(),
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
    pub is_verified: bool,
    pub card: MobileContactCard,
    pub added_at: u64,
}

impl From<&Contact> for MobileContact {
    fn from(contact: &Contact) -> Self {
        MobileContact {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            is_verified: contact.is_fingerprint_verified(),
            card: MobileContactCard::from(contact.card()),
            added_at: contact.exchange_timestamp(),
        }
    }
}

/// Exchange QR data.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileExchangeData {
    pub qr_data: String,
    pub public_id: String,
    pub expires_at: u64,
}

/// Exchange result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileExchangeResult {
    pub contact_id: String,
    pub contact_name: String,
    pub success: bool,
    pub error_message: Option<String>,
}

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSyncStatus {
    Idle,
    Syncing,
    Error,
}

/// Sync result with statistics.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSyncResult {
    /// Number of new contacts added from exchange messages.
    pub contacts_added: u32,
    /// Number of contact cards updated.
    pub cards_updated: u32,
    /// Number of outbound updates sent.
    pub updates_sent: u32,
}

/// Social network info.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSocialNetwork {
    pub id: String,
    pub display_name: String,
    pub url_template: String,
}

// === Recovery Types ===

/// Recovery claim data for mobile.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileRecoveryClaim {
    /// Old identity's public key (hex).
    pub old_public_key: String,
    /// New identity's public key (hex).
    pub new_public_key: String,
    /// Base64-encoded claim data.
    pub claim_data: String,
    /// Whether the claim has expired.
    pub is_expired: bool,
}

/// Recovery voucher data for mobile.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileRecoveryVoucher {
    /// Voucher public key (hex) - identifies who vouched.
    pub voucher_public_key: String,
    /// Base64-encoded voucher data.
    pub voucher_data: String,
}

/// Recovery progress status.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileRecoveryProgress {
    /// Old identity's public key (hex).
    pub old_public_key: String,
    /// New identity's public key (hex).
    pub new_public_key: String,
    /// Number of vouchers collected.
    pub vouchers_collected: u32,
    /// Number of vouchers needed (threshold).
    pub vouchers_needed: u32,
    /// Whether recovery is complete.
    pub is_complete: bool,
}

/// Recovery verification result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileRecoveryVerification {
    /// Old identity's public key (hex).
    pub old_public_key: String,
    /// New identity's public key (hex).
    pub new_public_key: String,
    /// Number of vouchers in the proof.
    pub voucher_count: u32,
    /// Number of vouchers from known contacts.
    pub known_vouchers: u32,
    /// Confidence level: "high", "medium", or "low".
    pub confidence: String,
    /// Recommendation for the user.
    pub recommendation: String,
}

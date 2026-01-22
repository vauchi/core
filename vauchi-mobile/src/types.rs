//! Mobile-friendly data types.
//!
//! These types are wrappers around vauchi-core types that are compatible
//! with UniFFI for cross-language bindings.

use vauchi_core::{Contact, ContactCard, ContactField, FieldType};

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

// === Visibility Label Types ===

/// Visibility label for organizing contacts.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileVisibilityLabel {
    /// Unique label ID.
    pub id: String,
    /// Human-readable label name.
    pub name: String,
    /// Number of contacts in this label.
    pub contact_count: u32,
    /// Number of visible fields for this label.
    pub visible_field_count: u32,
    /// Timestamp when created.
    pub created_at: u64,
    /// Timestamp when last modified.
    pub modified_at: u64,
}

impl From<&vauchi_core::VisibilityLabel> for MobileVisibilityLabel {
    fn from(label: &vauchi_core::VisibilityLabel) -> Self {
        MobileVisibilityLabel {
            id: label.id().to_string(),
            name: label.name().to_string(),
            contact_count: label.contact_count() as u32,
            visible_field_count: label.visible_fields().len() as u32,
            created_at: label.created_at(),
            modified_at: label.modified_at(),
        }
    }
}

/// Detailed label info including contacts and visible fields.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileVisibilityLabelDetail {
    /// Basic label info.
    pub id: String,
    pub name: String,
    /// Contact IDs in this label.
    pub contact_ids: Vec<String>,
    /// Field IDs visible to contacts in this label.
    pub visible_field_ids: Vec<String>,
    pub created_at: u64,
    pub modified_at: u64,
}

impl From<&vauchi_core::VisibilityLabel> for MobileVisibilityLabelDetail {
    fn from(label: &vauchi_core::VisibilityLabel) -> Self {
        MobileVisibilityLabelDetail {
            id: label.id().to_string(),
            name: label.name().to_string(),
            contact_ids: label.contacts().iter().cloned().collect(),
            visible_field_ids: label.visible_fields().iter().cloned().collect(),
            created_at: label.created_at(),
            modified_at: label.modified_at(),
        }
    }
}

// === Device Linking Types ===

/// Device link QR data for display on existing device.
#[allow(dead_code)]
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceLinkData {
    /// QR code content (base64-encoded link data).
    pub qr_data: String,
    /// Identity public key (hex).
    pub identity_public_key: String,
    /// Unix timestamp when QR was generated.
    pub timestamp: u64,
    /// Unix timestamp when QR expires.
    pub expires_at: u64,
}

/// Device link info parsed from QR code.
#[allow(dead_code)]
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceLinkInfo {
    /// Identity public key (hex).
    pub identity_public_key: String,
    /// Unix timestamp when QR was generated.
    pub timestamp: u64,
    /// Whether the QR code has expired.
    pub is_expired: bool,
}

/// Result of completing device link (for existing device).
#[allow(dead_code)]
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceLinkResult {
    /// Whether linking was successful.
    pub success: bool,
    /// New device's name.
    pub device_name: String,
    /// New device's index.
    pub device_index: u32,
    /// Error message if failed.
    pub error_message: Option<String>,
}

/// Device info for display.
#[allow(dead_code)]
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceInfo {
    /// Device index (0 = primary device).
    pub device_index: u32,
    /// Device name.
    pub device_name: String,
    /// Whether this is the current device.
    pub is_current: bool,
    /// Whether the device is active (not revoked).
    pub is_active: bool,
    /// Public key prefix (hex, first 16 chars).
    pub public_key_prefix: String,
}

// === Delivery Status Types ===

/// Delivery status for tracking message delivery progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileDeliveryStatus {
    /// Message queued locally, not yet sent.
    Queued,
    /// Message sent to relay.
    Sent,
    /// Relay confirmed storage.
    Stored,
    /// Recipient confirmed receipt.
    Delivered,
    /// Message expired without delivery.
    Expired,
    /// Delivery failed.
    Failed,
}

impl From<&vauchi_core::storage::DeliveryStatus> for MobileDeliveryStatus {
    fn from(status: &vauchi_core::storage::DeliveryStatus) -> Self {
        use vauchi_core::storage::DeliveryStatus;
        match status {
            DeliveryStatus::Queued => MobileDeliveryStatus::Queued,
            DeliveryStatus::Sent => MobileDeliveryStatus::Sent,
            DeliveryStatus::Stored => MobileDeliveryStatus::Stored,
            DeliveryStatus::Delivered => MobileDeliveryStatus::Delivered,
            DeliveryStatus::Expired => MobileDeliveryStatus::Expired,
            DeliveryStatus::Failed { .. } => MobileDeliveryStatus::Failed,
        }
    }
}

/// A record tracking delivery status of an outbound message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeliveryRecord {
    /// Unique message ID.
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// Current delivery status.
    pub status: MobileDeliveryStatus,
    /// Error reason if failed.
    pub error_reason: Option<String>,
    /// When the message was created (Unix timestamp).
    pub created_at: u64,
    /// When the status was last updated (Unix timestamp).
    pub updated_at: u64,
    /// When the message expires (Unix timestamp, optional).
    pub expires_at: Option<u64>,
}

impl From<&vauchi_core::storage::DeliveryRecord> for MobileDeliveryRecord {
    fn from(record: &vauchi_core::storage::DeliveryRecord) -> Self {
        use vauchi_core::storage::DeliveryStatus;
        let error_reason = match &record.status {
            DeliveryStatus::Failed { reason } => Some(reason.clone()),
            _ => None,
        };
        MobileDeliveryRecord {
            message_id: record.message_id.clone(),
            recipient_id: record.recipient_id.clone(),
            status: MobileDeliveryStatus::from(&record.status),
            error_reason,
            created_at: record.created_at,
            updated_at: record.updated_at,
            expires_at: record.expires_at,
        }
    }
}

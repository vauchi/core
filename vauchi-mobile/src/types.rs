//! Mobile-friendly data types.
//!
//! These types are wrappers around vauchi-core types that are compatible
//! with UniFFI for cross-language bindings.

use std::collections::HashMap;
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
    /// Unix timestamp when the device was created.
    pub created_at: u64,
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

// === Retry Queue Types ===

/// A retry queue entry for failed message deliveries.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileRetryEntry {
    /// Unique message ID.
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// Current retry attempt (0 = first attempt).
    pub attempt: u32,
    /// Unix timestamp for next retry.
    pub next_retry: u64,
    /// When the entry was created (Unix timestamp).
    pub created_at: u64,
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Whether max attempts have been exceeded.
    pub is_max_exceeded: bool,
}

impl From<&vauchi_core::storage::RetryEntry> for MobileRetryEntry {
    fn from(entry: &vauchi_core::storage::RetryEntry) -> Self {
        MobileRetryEntry {
            message_id: entry.message_id.clone(),
            recipient_id: entry.recipient_id.clone(),
            attempt: entry.attempt,
            next_retry: entry.next_retry,
            created_at: entry.created_at,
            max_attempts: entry.max_attempts,
            is_max_exceeded: entry.is_max_attempts_exceeded(),
        }
    }
}

// === Multi-Device Delivery Types ===

/// Delivery status for a specific device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileDeviceDeliveryStatus {
    /// Message pending delivery to this device.
    Pending,
    /// Message stored at relay for this device.
    Stored,
    /// Message delivered to this device.
    Delivered,
    /// Delivery to this device failed.
    Failed,
}

impl From<&vauchi_core::storage::DeviceDeliveryStatus> for MobileDeviceDeliveryStatus {
    fn from(status: &vauchi_core::storage::DeviceDeliveryStatus) -> Self {
        use vauchi_core::storage::DeviceDeliveryStatus;
        match status {
            DeviceDeliveryStatus::Pending => MobileDeviceDeliveryStatus::Pending,
            DeviceDeliveryStatus::Stored => MobileDeviceDeliveryStatus::Stored,
            DeviceDeliveryStatus::Delivered => MobileDeviceDeliveryStatus::Delivered,
            DeviceDeliveryStatus::Failed => MobileDeviceDeliveryStatus::Failed,
        }
    }
}

/// Per-device delivery tracking record.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceDeliveryRecord {
    /// Message ID being tracked.
    pub message_id: String,
    /// Recipient's contact ID.
    pub recipient_id: String,
    /// Target device ID.
    pub device_id: String,
    /// Delivery status for this device.
    pub status: MobileDeviceDeliveryStatus,
    /// When the status was last updated (Unix timestamp).
    pub updated_at: u64,
}

impl From<&vauchi_core::storage::DeviceDeliveryRecord> for MobileDeviceDeliveryRecord {
    fn from(record: &vauchi_core::storage::DeviceDeliveryRecord) -> Self {
        MobileDeviceDeliveryRecord {
            message_id: record.message_id.clone(),
            recipient_id: record.recipient_id.clone(),
            device_id: record.device_id.clone(),
            status: MobileDeviceDeliveryStatus::from(&record.status),
            updated_at: record.updated_at,
        }
    }
}

/// Summary of delivery status across all devices.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeliverySummary {
    /// Message ID.
    pub message_id: String,
    /// Total number of target devices.
    pub total_devices: u32,
    /// Number of devices that received the message.
    pub delivered_devices: u32,
    /// Number of devices still pending.
    pub pending_devices: u32,
    /// Number of devices where delivery failed.
    pub failed_devices: u32,
    /// Whether all devices have received the message.
    pub is_fully_delivered: bool,
    /// Progress as percentage (0-100).
    pub progress_percent: u32,
}

impl From<&vauchi_core::storage::DeliverySummary> for MobileDeliverySummary {
    fn from(summary: &vauchi_core::storage::DeliverySummary) -> Self {
        MobileDeliverySummary {
            message_id: summary.message_id.clone(),
            total_devices: summary.total_devices as u32,
            delivered_devices: summary.delivered_devices as u32,
            pending_devices: summary.pending_devices as u32,
            failed_devices: summary.failed_devices as u32,
            is_fully_delivered: summary.is_fully_delivered(),
            progress_percent: (summary.progress() * 100.0) as u32,
        }
    }
}

// === Aha Moment Types ===

/// Type of aha moment milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileAhaMomentType {
    /// Shown when card creation completes
    CardCreationComplete,
    /// Shown on first edit (before having contacts)
    FirstEdit,
    /// Shown when first contact is added
    FirstContactAdded,
    /// Shown when receiving first update from a contact
    FirstUpdateReceived,
    /// Shown when first outbound update is delivered
    FirstOutboundDelivered,
}

impl From<vauchi_core::AhaMomentType> for MobileAhaMomentType {
    fn from(t: vauchi_core::AhaMomentType) -> Self {
        match t {
            vauchi_core::AhaMomentType::CardCreationComplete => {
                MobileAhaMomentType::CardCreationComplete
            }
            vauchi_core::AhaMomentType::FirstEdit => MobileAhaMomentType::FirstEdit,
            vauchi_core::AhaMomentType::FirstContactAdded => MobileAhaMomentType::FirstContactAdded,
            vauchi_core::AhaMomentType::FirstUpdateReceived => {
                MobileAhaMomentType::FirstUpdateReceived
            }
            vauchi_core::AhaMomentType::FirstOutboundDelivered => {
                MobileAhaMomentType::FirstOutboundDelivered
            }
        }
    }
}

impl From<MobileAhaMomentType> for vauchi_core::AhaMomentType {
    fn from(t: MobileAhaMomentType) -> Self {
        match t {
            MobileAhaMomentType::CardCreationComplete => {
                vauchi_core::AhaMomentType::CardCreationComplete
            }
            MobileAhaMomentType::FirstEdit => vauchi_core::AhaMomentType::FirstEdit,
            MobileAhaMomentType::FirstContactAdded => vauchi_core::AhaMomentType::FirstContactAdded,
            MobileAhaMomentType::FirstUpdateReceived => {
                vauchi_core::AhaMomentType::FirstUpdateReceived
            }
            MobileAhaMomentType::FirstOutboundDelivered => {
                vauchi_core::AhaMomentType::FirstOutboundDelivered
            }
        }
    }
}

/// An aha moment to display to the user.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileAhaMoment {
    /// The type of milestone
    pub moment_type: MobileAhaMomentType,
    /// Title to display
    pub title: String,
    /// Message to display
    pub message: String,
    /// Whether to show animation
    pub has_animation: bool,
}

// === Demo Contact Types ===

/// Demo contact card representation for display.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDemoContact {
    /// Contact ID
    pub id: String,
    /// Display name
    pub display_name: String,
    /// Flag indicating this is a demo
    pub is_demo: bool,
    /// Current tip title
    pub tip_title: String,
    /// Current tip content
    pub tip_content: String,
    /// Tip category
    pub tip_category: String,
}

impl From<vauchi_core::DemoContactCard> for MobileDemoContact {
    fn from(card: vauchi_core::DemoContactCard) -> Self {
        MobileDemoContact {
            id: card.id,
            display_name: card.display_name,
            is_demo: card.is_demo,
            tip_title: card.tip_title,
            tip_content: card.tip_content,
            tip_category: card.tip_category,
        }
    }
}

/// Demo contact state for persistence.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDemoContactState {
    /// Whether the demo contact is active
    pub is_active: bool,
    /// Whether it was manually dismissed
    pub was_dismissed: bool,
    /// Whether it was auto-removed after first real exchange
    pub auto_removed: bool,
    /// Number of updates sent
    pub update_count: u32,
}

// === Field Validation Types ===

/// Trust level based on validation count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileTrustLevel {
    /// No validations yet.
    Unverified,
    /// 1 validation.
    LowConfidence,
    /// 2-4 validations.
    PartialConfidence,
    /// 5+ validations.
    HighConfidence,
}

impl From<vauchi_core::social::TrustLevel> for MobileTrustLevel {
    fn from(level: vauchi_core::social::TrustLevel) -> Self {
        match level {
            vauchi_core::social::TrustLevel::Unverified => MobileTrustLevel::Unverified,
            vauchi_core::social::TrustLevel::LowConfidence => MobileTrustLevel::LowConfidence,
            vauchi_core::social::TrustLevel::PartialConfidence => {
                MobileTrustLevel::PartialConfidence
            }
            vauchi_core::social::TrustLevel::HighConfidence => MobileTrustLevel::HighConfidence,
        }
    }
}

/// Validation status for a field.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileValidationStatus {
    /// Total number of validations.
    pub count: u32,
    /// Trust level based on count.
    pub trust_level: MobileTrustLevel,
    /// Trust level label for display.
    pub trust_level_label: String,
    /// Color indicator for UI (grey, yellow, light_green, green).
    pub color: String,
    /// Whether the current user has validated this field.
    pub validated_by_me: bool,
    /// Display text (e.g., "Verified by Bob and 2 others").
    pub display_text: String,
}

impl From<&vauchi_core::social::ValidationStatus> for MobileValidationStatus {
    fn from(status: &vauchi_core::social::ValidationStatus) -> Self {
        let known_names = std::collections::HashMap::new();
        MobileValidationStatus {
            count: status.count as u32,
            trust_level: status.trust_level.into(),
            trust_level_label: status.trust_level.label().to_string(),
            color: status.trust_level.color().to_string(),
            validated_by_me: status.validated_by_me,
            display_text: status.display(&known_names),
        }
    }
}

/// A validation record for a contact's field.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileFieldValidation {
    /// Contact ID that was validated.
    pub contact_id: String,
    /// Field name that was validated (e.g., "twitter", "email").
    pub field_name: String,
    /// Field value at time of validation.
    pub field_value: String,
    /// Timestamp when validation was created.
    pub validated_at: u64,
}

impl From<&vauchi_core::social::ProfileValidation> for MobileFieldValidation {
    fn from(validation: &vauchi_core::social::ProfileValidation) -> Self {
        MobileFieldValidation {
            contact_id: validation.contact_id().unwrap_or("unknown").to_string(),
            field_name: validation.field_name().unwrap_or("unknown").to_string(),
            field_value: validation.field_value().to_string(),
            validated_at: validation.validated_at(),
        }
    }
}

// ============================================================
// Theme Types
// ============================================================

/// Theme mode (light or dark)
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileThemeMode {
    Light,
    Dark,
}

impl From<vauchi_core::theme::ThemeMode> for MobileThemeMode {
    fn from(mode: vauchi_core::theme::ThemeMode) -> Self {
        match mode {
            vauchi_core::theme::ThemeMode::Light => MobileThemeMode::Light,
            vauchi_core::theme::ThemeMode::Dark => MobileThemeMode::Dark,
        }
    }
}

impl From<MobileThemeMode> for vauchi_core::theme::ThemeMode {
    fn from(mode: MobileThemeMode) -> Self {
        match mode {
            MobileThemeMode::Light => vauchi_core::theme::ThemeMode::Light,
            MobileThemeMode::Dark => vauchi_core::theme::ThemeMode::Dark,
        }
    }
}

/// Theme colors for UI styling.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileThemeColors {
    /// Primary background color (hex).
    pub bg_primary: String,
    /// Secondary background color (hex).
    pub bg_secondary: String,
    /// Tertiary background color (hex).
    pub bg_tertiary: String,
    /// Primary text color (hex).
    pub text_primary: String,
    /// Secondary text color (hex).
    pub text_secondary: String,
    /// Accent color (hex).
    pub accent: String,
    /// Dark accent color (hex).
    pub accent_dark: String,
    /// Success color (hex).
    pub success: String,
    /// Error color (hex).
    pub error: String,
    /// Warning color (hex).
    pub warning: String,
    /// Border color (hex).
    pub border: String,
}

impl From<&vauchi_core::theme::ThemeColors> for MobileThemeColors {
    fn from(colors: &vauchi_core::theme::ThemeColors) -> Self {
        MobileThemeColors {
            bg_primary: colors.bg_primary.clone(),
            bg_secondary: colors.bg_secondary.clone(),
            bg_tertiary: colors.bg_tertiary.clone(),
            text_primary: colors.text_primary.clone(),
            text_secondary: colors.text_secondary.clone(),
            accent: colors.accent.clone(),
            accent_dark: colors.accent_dark.clone(),
            success: colors.success.clone(),
            error: colors.error.clone(),
            warning: colors.warning.clone(),
            border: colors.border.clone(),
        }
    }
}

/// A complete theme definition.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTheme {
    /// Theme identifier.
    pub id: String,
    /// Theme display name.
    pub name: String,
    /// Theme version.
    pub version: String,
    /// Theme author (optional).
    pub author: Option<String>,
    /// Theme license (optional).
    pub license: Option<String>,
    /// Theme source URL (optional).
    pub source: Option<String>,
    /// Theme mode (light or dark).
    pub mode: MobileThemeMode,
    /// Theme colors.
    pub colors: MobileThemeColors,
}

impl From<&vauchi_core::theme::Theme> for MobileTheme {
    fn from(theme: &vauchi_core::theme::Theme) -> Self {
        MobileTheme {
            id: theme.id.clone(),
            name: theme.name.clone(),
            version: theme.version.clone(),
            author: theme.author.clone(),
            license: theme.license.clone(),
            source: theme.source.clone(),
            mode: theme.mode.into(),
            colors: MobileThemeColors::from(&theme.colors),
        }
    }
}

// ============================================================
// i18n Types
// ============================================================

/// Supported locales for the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileLocale {
    English,
    German,
    French,
    Spanish,
}

impl From<vauchi_core::i18n::Locale> for MobileLocale {
    fn from(locale: vauchi_core::i18n::Locale) -> Self {
        match locale {
            vauchi_core::i18n::Locale::English => MobileLocale::English,
            vauchi_core::i18n::Locale::German => MobileLocale::German,
            vauchi_core::i18n::Locale::French => MobileLocale::French,
            vauchi_core::i18n::Locale::Spanish => MobileLocale::Spanish,
        }
    }
}

impl From<MobileLocale> for vauchi_core::i18n::Locale {
    fn from(locale: MobileLocale) -> Self {
        match locale {
            MobileLocale::English => vauchi_core::i18n::Locale::English,
            MobileLocale::German => vauchi_core::i18n::Locale::German,
            MobileLocale::French => vauchi_core::i18n::Locale::French,
            MobileLocale::Spanish => vauchi_core::i18n::Locale::Spanish,
        }
    }
}

/// Information about a locale.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileLocaleInfo {
    /// ISO 639-1 language code.
    pub code: String,
    /// Native name of the language.
    pub name: String,
    /// English name of the language.
    pub english_name: String,
    /// Whether the language is right-to-left.
    pub is_rtl: bool,
}

impl From<vauchi_core::i18n::LocaleInfo> for MobileLocaleInfo {
    fn from(info: vauchi_core::i18n::LocaleInfo) -> Self {
        MobileLocaleInfo {
            code: info.code.to_string(),
            name: info.name.to_string(),
            english_name: info.english_name.to_string(),
            is_rtl: info.is_rtl,
        }
    }
}

// ============================================================
// Help Types
// ============================================================

/// Categories of help content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileHelpCategory {
    GettingStarted,
    Privacy,
    Recovery,
    Contacts,
    Updates,
    Features,
}

impl From<vauchi_core::help::HelpCategory> for MobileHelpCategory {
    fn from(category: vauchi_core::help::HelpCategory) -> Self {
        match category {
            vauchi_core::help::HelpCategory::GettingStarted => MobileHelpCategory::GettingStarted,
            vauchi_core::help::HelpCategory::Privacy => MobileHelpCategory::Privacy,
            vauchi_core::help::HelpCategory::Recovery => MobileHelpCategory::Recovery,
            vauchi_core::help::HelpCategory::Contacts => MobileHelpCategory::Contacts,
            vauchi_core::help::HelpCategory::Updates => MobileHelpCategory::Updates,
            vauchi_core::help::HelpCategory::Features => MobileHelpCategory::Features,
        }
    }
}

impl From<MobileHelpCategory> for vauchi_core::help::HelpCategory {
    fn from(category: MobileHelpCategory) -> Self {
        match category {
            MobileHelpCategory::GettingStarted => vauchi_core::help::HelpCategory::GettingStarted,
            MobileHelpCategory::Privacy => vauchi_core::help::HelpCategory::Privacy,
            MobileHelpCategory::Recovery => vauchi_core::help::HelpCategory::Recovery,
            MobileHelpCategory::Contacts => vauchi_core::help::HelpCategory::Contacts,
            MobileHelpCategory::Updates => vauchi_core::help::HelpCategory::Updates,
            MobileHelpCategory::Features => vauchi_core::help::HelpCategory::Features,
        }
    }
}

/// Help category with display name.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileHelpCategoryInfo {
    /// Category identifier.
    pub category: MobileHelpCategory,
    /// Display name for the category.
    pub display_name: String,
}

/// A frequently asked question with answer.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileFaqItem {
    /// Unique identifier.
    pub id: String,
    /// Category this FAQ belongs to.
    pub category: MobileHelpCategory,
    /// The question.
    pub question: String,
    /// The answer (may contain markdown).
    pub answer: String,
    /// Related FAQ IDs.
    pub related: Vec<String>,
}

impl From<&vauchi_core::help::FaqItem> for MobileFaqItem {
    fn from(faq: &vauchi_core::help::FaqItem) -> Self {
        MobileFaqItem {
            id: faq.id.clone(),
            category: faq.category.into(),
            question: faq.question.clone(),
            answer: faq.answer.clone(),
            related: faq.related.clone(),
        }
    }
}

// ============================================================
// i18n Helper Functions (for types module)
// ============================================================

/// Get a localized string by key.
pub fn mobile_get_string(locale: MobileLocale, key: String) -> String {
    vauchi_core::i18n::get_string(locale.into(), &key)
}

/// Get a localized string with argument interpolation.
pub fn mobile_get_string_with_args(
    locale: MobileLocale,
    key: String,
    args: HashMap<String, String>,
) -> String {
    let args_vec: Vec<(&str, &str)> = args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    vauchi_core::i18n::get_string_with_args(locale.into(), &key, &args_vec)
}

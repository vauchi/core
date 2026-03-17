// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social, visibility, and trust types.
//!
//! Social networks, visibility labels, trust levels, and field validation.

/// Social network info.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSocialNetwork {
    pub id: String,
    pub display_name: String,
    pub url_template: String,
}

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

impl From<&vauchi_core::Group> for MobileVisibilityLabel {
    fn from(label: &vauchi_core::Group) -> Self {
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

impl From<&vauchi_core::Group> for MobileVisibilityLabelDetail {
    fn from(label: &vauchi_core::Group) -> Self {
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

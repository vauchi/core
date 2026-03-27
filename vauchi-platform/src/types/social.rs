// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social and visibility types.
//!
//! Social networks and visibility labels.

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

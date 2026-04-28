// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social and visibility types.
//!
//! Social networks and visibility labels.

use super::MobileContactTrustLevel;

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

/// Status of a contact reference within a label.
///
/// Today only `Active` is emitted — deleted-contact references are omitted
/// from `label_contacts` and counted in `stale_reference_count` instead.
/// Future variants may distinguish stale/tombstoned/etc. without a binding
/// break thanks to UniFFI's enum-extension semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileLabelContactStatus {
    /// The contact resolves to an active contact in storage.
    Active,
}

/// A status badge to render next to a label-contact row.
///
/// Mirrors `MobileContactDetailBadge` so the LabelDetail and
/// ContactDetail screens share the same canonical predicate set —
/// frontends iterate the list, never branching on raw `MobileContact`
/// flags (ADR-021/043 Humble UI). Future variants may surface
/// recovery-trust or other typed signals without a binding break
/// thanks to UniFFI's enum-extension semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileLabelContactBadge {
    /// Fingerprint manually verified out-of-band — the iOS / Android
    /// LabelDetail screens render a checkmark next to the row.
    Verified,
}

/// A resolved contact row for a visibility label.
///
/// Frontends should render `MobileVisibilityLabelDetail.label_contacts`
/// instead of joining `contact_ids` against the contacts list themselves
/// (ADR-021/043 Humble UI). Display name resolution honors nicknames,
/// shared-name preferences, and avatar preferences — same pipeline used by
/// `enrich_contact()` for `list_contacts`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileLabelContactRow {
    /// Contact ID — same as the matching entry in `contact_ids`.
    pub id: String,
    /// Display name resolved per the user's nickname/shared-name preferences.
    pub display_name: String,
    /// Cryptographically-derived trust level (never user-editable).
    pub trust_level: MobileContactTrustLevel,
    /// Status of this row in the label (today always `Active`).
    pub status: MobileLabelContactStatus,
    /// Status badges for the row. Populated by `get_label`. Closes the
    /// G6 follow-up — restores the verified-checkmark dropped during the
    /// G4 ContactDetail consumer migration so iOS / Android LabelDetail
    /// can render it from typed data instead of branching on a raw
    /// `MobileContact` field.
    pub badges: Vec<MobileLabelContactBadge>,
}

/// Detailed label info including contacts and visible fields.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileVisibilityLabelDetail {
    /// Basic label info.
    pub id: String,
    pub name: String,
    /// Raw contact IDs in this label — superseded by `label_contacts`.
    ///
    /// Retained for backwards compatibility for one binding cycle so
    /// existing consumers don't break. Frontends should render
    /// `label_contacts` instead — that list is pre-resolved against
    /// storage so missing contacts cannot leak raw IDs into the UI.
    pub contact_ids: Vec<String>,
    /// Field IDs visible to contacts in this label.
    pub visible_field_ids: Vec<String>,
    pub created_at: u64,
    pub modified_at: u64,
    /// Resolved contact rows for the UI to render.
    ///
    /// Populated by `VauchiPlatform::get_label`; the bare `From<&Group>`
    /// impl leaves this empty (Group does not carry storage access).
    /// Order matches `contact_ids` insertion order.
    pub label_contacts: Vec<MobileLabelContactRow>,
    /// Number of `contact_ids` that did not resolve to an active contact.
    ///
    /// When `> 0`, the label references contacts that were removed from
    /// storage. UI may surface this as e.g. "(N stale references)".
    /// `label_contacts.len() + stale_reference_count == contact_ids.len()`
    /// is an invariant on the resolver output (verified in
    /// `mobile_visibility_resolve_tests`).
    pub stale_reference_count: u32,
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
            // Populated by VauchiPlatform::get_label after construction —
            // Group does not carry storage access. See mobile_visibility.rs.
            label_contacts: Vec::new(),
            stale_reference_count: 0,
        }
    }
}

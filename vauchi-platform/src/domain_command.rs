// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Long-tail domain dispatch for `PlatformAppEngine`.
//!
//! Phase B7 of `_private/docs/problems/2026-04-28-collapse-vauchi-platform-into-app-engine/`.
//! Replaces ~150 thin pass-through wrappers on `VauchiPlatform` with a
//! single typed entry point on `PlatformAppEngine`:
//!
//! ```ignore
//! engine.dispatch_domain_command(DomainCommand::GrantConsent {
//!     consent_type: MobileConsentType::Crashlytics,
//! })?;
//! ```
//!
//! The R3 hybrid (B1-CLASSIFICATION.md, accepted 2026-04-28) keeps
//! Recovery / Emergency Broadcast / Device Linking as direct typed
//! methods on `PlatformAppEngine` (B2/B3/B4); the long-tail collapses
//! into the [`DomainCommand`] enum here. Each domain is added in its
//! own batch MR rather than committing all 150 variants at once — this
//! keeps reviewer scope tractable and lets the typed-method pattern
//! validate before the enum bloats.
//!
//! The first batch is **Consent** (5 variants, this MR). Subsequent
//! batches will extend [`DomainCommand`] and [`DomainCommandResult`]
//! with new variants without breaking existing call sites — UniFFI
//! treats added enum cases as additive on the binding side.

use crate::content::{MobileApplyResult, MobileUpdateStatus};
use crate::types::{
    MobileConsentRecord, MobileConsentStatus, MobileConsentType, MobileSocialNetwork,
    MobileVisibilityLabel, MobileVisibilityLabelDetail,
};

/// Typed dispatch envelope for `PlatformAppEngine` operations that
/// don't justify their own `#[uniffi::export]` method.
///
/// Add a new variant per domain method that previously lived on
/// `VauchiPlatform`. Variant naming convention: `<Verb><Noun>`
/// (e.g., `GrantConsent`, `CheckConsent`) — matches the underlying
/// API method name where possible.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum DomainCommand {
    // ── Consent (B7 batch 1, this MR) ──
    /// Grant consent for a specific consent type.
    GrantConsent { consent_type: MobileConsentType },
    /// Revoke consent for a specific consent type.
    RevokeConsent { consent_type: MobileConsentType },
    /// Check whether consent is currently granted.
    CheckConsent { consent_type: MobileConsentType },
    /// Aggregated consent status (granted, last change, policy version).
    GetConsentStatus { consent_type: MobileConsentType },
    /// All persisted consent records.
    GetConsentRecords,

    // ── Content Updates (B7 batch 2, this MR) ──
    /// Returns `true` when the `content-updates` Cargo feature is
    /// enabled at compile time.
    IsContentUpdatesSupported,
    /// Check the remote update server for available content updates.
    /// Blocking — returns `Disabled` when the feature is off.
    CheckContentUpdates,
    /// Download and cache available updates. Returns the per-type
    /// outcome (applied vs failed). `Disabled` when the feature is off.
    ApplyContentUpdates,
    /// Reload the social-networks list from the content cache after
    /// `ApplyContentUpdates` succeeds.
    ReloadSocialNetworks,

    // ── Visibility Labels + Field Visibility (B7 batch 6) ──
    /// List every visibility label.
    ListLabels,
    /// Create a new label by name.
    CreateLabel { name: String },
    /// Read a label by id, including resolved contact rows.
    GetLabel { label_id: String },
    /// Rename a label.
    RenameLabel { label_id: String, new_name: String },
    /// Delete a label.
    DeleteLabel { label_id: String },
    /// Add a contact to a label.
    AddContactToGroup {
        label_id: String,
        contact_id: String,
    },
    /// Remove a contact from a label.
    RemoveContactFromGroup {
        label_id: String,
        contact_id: String,
    },
    /// List labels that contain a contact.
    GetGroupsForContact { contact_id: String },
    /// Set whether a card field is visible to contacts in a label.
    SetGroupFieldVisibility {
        label_id: String,
        field_label: String,
        is_visible: bool,
    },
    /// Set a per-contact override for field visibility.
    SetContactFieldOverride {
        contact_id: String,
        field_label: String,
        is_visible: bool,
    },
    /// Remove a per-contact field-visibility override.
    RemoveContactFieldOverride {
        contact_id: String,
        field_label: String,
    },
    /// Hide a field from a specific contact (sets the visibility rule
    /// on the contact's `visibility_rules`).
    HideFieldFromContact {
        contact_id: String,
        field_label: String,
    },
    /// Show a field to a specific contact.
    ShowFieldToContact {
        contact_id: String,
        field_label: String,
    },
    /// Read whether a field is visible to a specific contact.
    IsFieldVisibleToContact {
        contact_id: String,
        field_label: String,
    },
    /// Suggested default labels (from `vauchi_core::SUGGESTED_LABELS`).
    GetSuggestedLabels,
}

/// Sum type of every legitimate return shape from
/// [`DomainCommand`] dispatch. Pattern match on the variant your
/// command produces; mismatched variants in caller code indicate a
/// bug, not a runtime error.
///
/// Adding a new variant is non-breaking on the UniFFI binding side
/// — Swift / Kotlin pattern matches gain a new case in
/// `@unknown default` form. Removing a variant IS breaking; treat
/// removal as a major-version event.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum DomainCommandResult {
    /// Command returned `()` (write-path with no payload).
    Unit,
    /// Boolean result (`check_consent` etc.).
    Bool { value: bool },
    /// Aggregated `MobileConsentStatus` (B7 batch 1).
    ConsentStatus { status: MobileConsentStatus },
    /// List of `MobileConsentRecord` (B7 batch 1).
    ConsentRecords { records: Vec<MobileConsentRecord> },
    /// Outcome of `CheckContentUpdates` (B7 batch 2).
    UpdateStatus { status: MobileUpdateStatus },
    /// Outcome of `ApplyContentUpdates` (B7 batch 2).
    ApplyResult { result: MobileApplyResult },
    /// List of `MobileSocialNetwork` (B7 batch 2 — `ReloadSocialNetworks`).
    SocialNetworks { networks: Vec<MobileSocialNetwork> },
    /// List of visibility labels (B7 batch 6 — `ListLabels`,
    /// `GetGroupsForContact`).
    Labels { labels: Vec<MobileVisibilityLabel> },
    /// Single visibility label (B7 batch 6 — `CreateLabel`).
    Label { label: MobileVisibilityLabel },
    /// Visibility label with resolved contact rows (B7 batch 6 —
    /// `GetLabel`).
    LabelDetail { detail: MobileVisibilityLabelDetail },
    /// List of `String` payload (B7 batch 6 — `GetSuggestedLabels`).
    Strings { values: Vec<String> },
}

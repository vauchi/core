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
    MobileAhaMoment, MobileAhaMomentType, MobileConsentRecord, MobileConsentStatus,
    MobileConsentType, MobileContact, MobileContactCard, MobileDeletionInfo, MobileDemoContact,
    MobileDemoContactState, MobileFieldType, MobileGdprExport, MobileRecoveryVerification,
    MobileShredStatus, MobileSocialNetwork,
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

    // ── Content Updates (B7 batch 2) ──
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

    // ── GDPR / Deletion + read-only shred status (B7 batch 3) ──
    //
    // Note: the 5 keychain-bound shred methods (panic_shred, soft_shred,
    // hard_shred, cancel_shred, verify_shred) are NOT in this batch —
    // they require platform-keychain plumbing that PlatformAppEngine
    // doesn't have yet. Tracked as a separate B7 batch.
    /// Export all user data as JSON (GDPR right-to-export).
    ExportGdprData,
    /// Schedule identity deletion with a 7-day grace period.
    ScheduleIdentityDeletion,
    /// Cancel a scheduled identity deletion during the grace period.
    CancelIdentityDeletion,
    /// Execute a scheduled identity deletion after the grace period.
    /// Returns the count of revocations queued.
    ExecuteIdentityDeletion,
    /// Read the current deletion state.
    GetDeletionState,
    /// Read-only shred-process status. Mirrors the legacy
    /// `VauchiPlatform::shred_status` — does NOT require keychain.
    ShredStatus,

    // ── Aha Moments (B7 batch 5) ──
    /// Read whether the user has already seen a given milestone.
    HasSeenAhaMoment { moment_type: MobileAhaMomentType },
    /// Try to trigger a milestone if not yet seen. Returns the moment
    /// payload (title / message / animation flag) on first trigger,
    /// `None` once seen.
    TryTriggerAhaMoment { moment_type: MobileAhaMomentType },
    /// Like `TryTriggerAhaMoment` but with a context string (e.g. a
    /// contact name) substituted into the message template.
    TryTriggerAhaMomentWithContext {
        moment_type: MobileAhaMomentType,
        context: String,
    },
    /// Count of milestones already seen by the user.
    AhaMomentsSeenCount,
    /// Total count of milestones defined in core.
    AhaMomentsTotalCount,
    /// Reset every milestone to "unseen" (debug / settings affordance).
    ResetAhaMoments,

    // ── Demo Contact (B7 batch 5) ──
    /// Initialize the demo contact if the user has zero real contacts
    /// and never dismissed/auto-removed the demo. Idempotent.
    InitDemoContactIfNeeded,
    /// Read the current active demo contact, if any.
    GetDemoContact,
    /// Read the demo-contact tracker state.
    GetDemoContactState,
    /// Read whether a demo update is due.
    IsDemoUpdateAvailable,
    /// Advance the demo contact to the next tip and persist the state.
    TriggerDemoUpdate,
    /// Mark the demo contact as user-dismissed.
    DismissDemoContact,
    /// Mark the demo contact as auto-removed (after first real exchange).
    AutoRemoveDemoContact,
    /// Restore a previously-dismissed demo contact.
    RestoreDemoContact,

    // ── Contact Card + CRUD (B7 batch 10) ──
    /// Read the active identity's own contact card.
    GetOwnCard,
    /// Append a field to the own card.
    AddField {
        field_type: MobileFieldType,
        label: String,
        value: String,
    },
    /// Update an existing field's value (looked up by label).
    UpdateField { label: String, new_value: String },
    /// Remove a field by label. Returns `true` if it existed.
    RemoveField { label: String },
    /// Set the own card's display name.
    SetDisplayName { name: String },
    /// Replace the own card's avatar (any common image format,
    /// normalised to WebP ≤ 32 KB by core).
    SetOwnAvatar { avatar_bytes: Vec<u8> },
    /// Clear the own card's avatar.
    ClearOwnAvatar,

    // ── Contact CRUD (B7 batch 10) ──
    /// List every contact (enriched with display-name + avatar
    /// resolution).
    ListContacts,
    /// Read a single contact by id (enriched).
    GetContact { id: String },
    /// SQL-level search across contacts.
    SearchContacts { query: String },
    /// Total contact count.
    ContactCount,
    /// Hard-delete an exchanged contact. Returns `true` if removed.
    RemoveContact { id: String },
    /// Soft-delete an imported contact (keeps it in trash).
    SoftDeleteImportedContact { id: String },
    /// Undo a soft-delete on an imported contact.
    UndoDeleteImportedContact { id: String },
    /// Hard-delete an imported contact (no undo).
    HardDeleteImportedContact { id: String },
    /// Move an exchanged contact to the archive.
    ArchiveContact { id: String },
    /// Restore an archived contact to the active list.
    UnarchiveContact { id: String },
    /// List archived contacts (enriched).
    ListArchivedContacts,
    /// Hide a contact (keeps record but excludes from default views).
    HideContact { contact_id: String },
    /// Unhide a contact.
    UnhideContact { contact_id: String },

    // ── Recovery leftovers (B7 batch 4 — completes the recovery
    // domain; B2 covered the main 9 typed methods, this batch covers
    // the 3 long-tail methods that don't justify their own
    // PlatformAppEngine surface). ──
    /// Verify a recovery proof from a contact and produce a confidence
    /// recommendation (high / medium / low) based on known vouchers.
    VerifyRecoveryProof { proof_b64: String },
    /// Upload encrypted guardian entries (one per recovery-trusted
    /// contact) to the relay. Called after `trust_contact_for_recovery`
    /// or `untrust_contact_for_recovery` toggles the trust set.
    UploadGuardianEntries,
    /// Persist a user's recovery response (accept / reject /
    /// remind_me_later). Used by the `RecoveryClaimReviewEngine` to
    /// store the decision when the user reviews an incoming claim.
    SaveRecoveryResponse {
        claim_id: String,
        contact_id: String,
        response: String,
        remind_at: Option<u64>,
    },
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
    /// Numeric `u32` result — used by both B7 batch 3
    /// (`ExecuteIdentityDeletion` revocation count) and B7 batch 5
    /// (`AhaMomentsSeenCount`, `AhaMomentsTotalCount`).
    Count { value: u32 },
    /// GDPR export payload (B7 batch 3 — `ExportGdprData`).
    GdprExport { export: MobileGdprExport },
    /// Deletion-state snapshot (B7 batch 3 — `ScheduleIdentityDeletion`,
    /// `GetDeletionState`).
    DeletionInfo { info: MobileDeletionInfo },
    /// Shred-process status snapshot (B7 batch 3 — `ShredStatus`).
    ShredStatus { status: MobileShredStatus },
    /// Optional aha-moment payload (B7 batch 5 —
    /// `TryTriggerAhaMoment` and friends).
    AhaMomentOpt { moment: Option<MobileAhaMoment> },
    /// Optional demo-contact payload (B7 batch 5 —
    /// `InitDemoContactIfNeeded`, `GetDemoContact`,
    /// `TriggerDemoUpdate`, `RestoreDemoContact`).
    DemoContactOpt { contact: Option<MobileDemoContact> },
    /// Demo-contact tracker state snapshot (B7 batch 5 —
    /// `GetDemoContactState`).
    DemoContactState { state: MobileDemoContactState },
    /// Own contact card (B7 batch 10 — `GetOwnCard`).
    ContactCardPayload { card: MobileContactCard },
    /// Optional contact (B7 batch 10 — `GetContact`).
    ContactOpt { contact: Option<MobileContact> },
    /// List of contacts (B7 batch 10 — `ListContacts`,
    /// `SearchContacts`, `ListArchivedContacts`).
    Contacts { contacts: Vec<MobileContact> },
    /// Recovery-proof verification result (B7 batch 4 —
    /// `VerifyRecoveryProof`).
    RecoveryVerification {
        verification: MobileRecoveryVerification,
    },
}

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
    MobileConsentType, MobileDemoContact, MobileDemoContactState, MobileSocialNetwork,
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
    /// Numeric `u32` result (B7 batch 5 — `AhaMomentsSeenCount`,
    /// `AhaMomentsTotalCount`).
    Count { value: u32 },
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
}

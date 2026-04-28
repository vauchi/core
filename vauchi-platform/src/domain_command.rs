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
    MobileConsentRecord, MobileConsentStatus, MobileConsentType, MobileDeliveryRecord,
    MobileDeliveryStatus, MobileDeliverySummary, MobileDeviceDeliveryRecord, MobileRetryEntry,
    MobileSocialNetwork,
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

    // ── Sync / Delivery / Retry — read paths + simple writes (B7 batch 8) ──
    //
    // The state-heavy methods (sync, get_sync_status, delivery
    // receipts / suppress-presence flags, backup export/import) need
    // engine-resident state and are deferred to a separate "sync
    // orchestration" batch. This batch covers the 21 storage-only
    // delegations that translate cleanly into dispatch arms today.
    /// Total pending updates across all contacts.
    PendingUpdateCount,
    /// Read a delivery record by message id.
    GetDeliveryRecord { message_id: String },
    /// All delivery records.
    GetAllDeliveryRecords,
    /// Delivery records for a specific recipient.
    GetDeliveryRecordsForContact { recipient_id: String },
    /// Count of failed deliveries.
    CountFailedDeliveries,
    /// All failed delivery records.
    GetFailedDeliveryRecords,
    /// Reschedule a failed delivery for immediate retry.
    /// Returns `true` if the retry entry was found and rescheduled.
    ManualRetry { message_id: String },
    /// All non-terminal pending deliveries.
    GetPendingDeliveries,
    /// Count of deliveries in a specific status.
    GetDeliveryCountByStatus { status: MobileDeliveryStatus },
    /// All retry entries due for retry now.
    GetDueRetries,
    /// Retry entries for a specific contact.
    GetRetriesForContact { contact_id: String },
    /// Total count of retry entries.
    GetRetryCount,
    /// Delete a retry entry by message id.
    DeleteRetry { message_id: String },
    /// Compute the backoff (seconds) for a retry attempt. Pure.
    CalculateRetryBackoff { attempt: u32 },
    /// Total pending updates count (alias for `PendingUpdateCount`'s
    /// alternative implementation — counts via storage directly).
    GetTotalPendingCount,
    /// Whether the offline queue is at capacity.
    IsOfflineQueueFull,
    /// Remaining capacity in the offline queue.
    GetOfflineQueueCapacity,
    /// Drop all pending updates for a contact. Returns the cleared
    /// count.
    ClearPendingUpdatesForContact { contact_id: String },
    /// Multi-device delivery summary for a message.
    GetDeliverySummary { message_id: String },
    /// All device delivery records for a message.
    GetDeviceDeliveries { message_id: String },
    /// All pending device deliveries.
    GetPendingDeviceDeliveries,
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
    /// Numeric u32 result (B7 batch 8 — `*Count*` commands).
    Count { value: u32 },
    /// Numeric u64 result (B7 batch 8 — `CalculateRetryBackoff`).
    BackoffSeconds { seconds: u64 },
    /// Optional delivery record (B7 batch 8 — `GetDeliveryRecord`).
    DeliveryRecordOpt {
        record: Option<MobileDeliveryRecord>,
    },
    /// List of delivery records (B7 batch 8 — multiple).
    DeliveryRecords { records: Vec<MobileDeliveryRecord> },
    /// List of retry entries (B7 batch 8).
    RetryEntries { entries: Vec<MobileRetryEntry> },
    /// Multi-device delivery summary (B7 batch 8 — `GetDeliverySummary`).
    DeliverySummary { summary: MobileDeliverySummary },
    /// List of device delivery records (B7 batch 8 —
    /// `GetDeviceDeliveries`, `GetPendingDeviceDeliveries`).
    DeviceDeliveries {
        records: Vec<MobileDeviceDeliveryRecord>,
    },
}

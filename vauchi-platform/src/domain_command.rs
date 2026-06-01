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
use crate::mobile_contact_detail::MobileContactDetailViewState;
use crate::mobile_import::MobileImportResult;
use crate::types::{
    MobileAhaMoment, MobileAhaMomentType, MobileAuthMode, MobileBroadcastResult,
    MobileConsentRecord, MobileConsentStatus, MobileConsentType, MobileContact, MobileContactCard,
    MobileContactDisplayOptions, MobileDecoyContact, MobileDeletionInfo, MobileDeliveryRecord,
    MobileDeliveryStatus, MobileDeliverySummary, MobileDemoContact, MobileDemoContactState,
    MobileDeviceDeliveryRecord, MobileDeviceInfo, MobileDeviceLinkData, MobileDeviceLinkInfo,
    MobileDuplicatePair, MobileDuressSettings, MobileEmergencyConfig, MobileFieldNote,
    MobileFieldType, MobileGdprExport, MobileOnboardingProgress, MobileOnboardingStep,
    MobileRecoveryClaim, MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher,
    MobileRetryEntry, MobileShredReport, MobileShredStatus, MobileShredToken, MobileSocialNetwork,
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
    GrantConsent {
        consent_type: MobileConsentType,
    },
    /// Revoke consent for a specific consent type.
    RevokeConsent {
        consent_type: MobileConsentType,
    },
    /// Check whether consent is currently granted.
    CheckConsent {
        consent_type: MobileConsentType,
    },
    /// Aggregated consent status (granted, last change, policy version).
    GetConsentStatus {
        consent_type: MobileConsentType,
    },
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
    // Keychain-bound shred (B7 keychain batch): all four variants are
    // wired below, building `ShredManager` from the PAE keychain bridge.
    // `SoftShred` / `CancelShred` (Phase 1a) touch only storage (no SMK
    // destruction); `HardShred` / `PanicShred` (Phase 1b) additionally
    // run relay purge/revocation senders + SMK destruction.
    // (verify_shred retired 2026-05-23 Track A.)
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
    /// Schedule crypto-shredding with a grace period (Soft Shred).
    /// Requires a platform keychain set via `set_platform_keychain`.
    /// Returns the `MobileShredToken` authorising a later `HardShred`.
    SoftShred,
    /// Cancel a scheduled soft-shred during the grace period.
    /// Requires a platform keychain and the token from `SoftShred`.
    CancelShred {
        token: MobileShredToken,
    },
    /// Execute irreversible crypto-shredding after the grace period
    /// (Hard Shred). Requires the token from `SoftShred` + a keychain.
    /// Destroys key material, the database, and local data; best-effort
    /// purge + revocation to the relay.
    HardShred {
        token: MobileShredToken,
    },
    /// Immediate irreversible crypto-shredding with no grace period
    /// (Panic Shred). Requires a platform keychain.
    PanicShred,

    // ── Aha Moments (B7 batch 5) ──
    /// Read whether the user has already seen a given milestone.
    HasSeenAhaMoment {
        moment_type: MobileAhaMomentType,
    },
    /// Try to trigger a milestone if not yet seen. Returns the moment
    /// payload (title / message / animation flag) on first trigger,
    /// `None` once seen.
    TryTriggerAhaMoment {
        moment_type: MobileAhaMomentType,
    },
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
    UpdateField {
        label: String,
        new_value: String,
    },
    /// Remove a field by label. Returns `true` if it existed.
    RemoveField {
        label: String,
    },
    /// Set the own card's display name.
    SetDisplayName {
        name: String,
    },
    /// Replace the own card's avatar (any common image format,
    /// normalised to WebP ≤ 32 KB by core).
    SetOwnAvatar {
        avatar_bytes: Vec<u8>,
    },
    /// Clear the own card's avatar.
    ClearOwnAvatar,

    // ── Contact CRUD (B7 batch 10) ──
    /// List every contact (enriched with display-name + avatar
    /// resolution).
    ListContacts,
    /// Read a single contact by id (enriched).
    GetContact {
        id: String,
    },
    /// SQL-level search across contacts.
    SearchContacts {
        query: String,
    },
    /// Total contact count.
    ContactCount,
    /// Hard-delete an exchanged contact. Returns `true` if removed.
    RemoveContact {
        id: String,
    },
    /// Soft-delete an imported contact (keeps it in trash).
    SoftDeleteImportedContact {
        id: String,
    },
    /// Undo a soft-delete on an imported contact.
    UndoDeleteImportedContact {
        id: String,
    },
    /// Hard-delete an imported contact (no undo).
    HardDeleteImportedContact {
        id: String,
    },
    /// Move an exchanged contact to the archive.
    ArchiveContact {
        id: String,
    },
    /// Restore an archived contact to the active list.
    UnarchiveContact {
        id: String,
    },
    /// List archived contacts (enriched).
    ListArchivedContacts,
    /// Hide a contact (keeps record but excludes from default views).
    HideContact {
        contact_id: String,
    },
    /// Unhide a contact.
    UnhideContact {
        contact_id: String,
    },

    // ── Recovery leftovers (B7 batch 4 — completes the recovery
    // domain; B2 covered the main 9 typed methods, this batch covers
    // the 3 long-tail methods that don't justify their own
    // PlatformAppEngine surface). ──
    VerifyRecoveryProof {
        proof_b64: String,
    },
    UploadGuardianEntries,
    SaveRecoveryResponse {
        claim_id: String,
        contact_id: String,
        response: String,
        remind_at: Option<u64>,
    },
    // ── Recovery-trust toggle + count (slice 32g-B prep — mirrors the
    // three pub fns previously duplicated on `PlatformAppEngine`
    // (trust_contact_for_recovery / untrust / trusted_contact_count)
    // and on `mobile_contacts.rs::impl VauchiPlatform`. Adding them as
    // DomainCommands lets slice 32g retire both copies atomically and
    // gives iOS a uniform dispatch entry point.) ──
    TrustContactForRecovery {
        contact_id: String,
    },
    UntrustContactForRecovery {
        contact_id: String,
    },
    TrustedContactCount,

    // ── Recovery typed-method retirement (Track B B4a) ──
    ParseRecoveryClaim {
        claim_b64: String,
    },
    GetRecoveryProof,
    GetRecoveryStatus,
    CreateRecoveryVoucher {
        claim_b64: String,
    },
    AddRecoveryVoucher {
        voucher_b64: String,
    },
    CreateRecoveryClaim {
        old_pk_hex: String,
    },

    // ── Emergency-broadcast typed-method retirement (Track B B4b) ──
    ConfigureEmergencyBroadcast {
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    },
    SendEmergencyBroadcast,
    GetEmergencyConfig,
    DisableEmergencyBroadcast,
    // ── Visibility Labels + Field Visibility (B7 batch 6) ──
    /// List every visibility label.
    ListLabels,
    /// Create a new label by name.
    CreateLabel {
        name: String,
    },
    /// Read a label by id, including resolved contact rows.
    GetLabel {
        label_id: String,
    },
    /// Rename a label.
    RenameLabel {
        label_id: String,
        new_name: String,
    },
    /// Delete a label.
    DeleteLabel {
        label_id: String,
    },
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
    GetGroupsForContact {
        contact_id: String,
    },
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
    // ── Passcode + Duress + Decoy (B7 batch 7) ──
    /// Set up the app password (PIN). Requires identity.
    SetupAppPassword {
        password: String,
    },
    /// Set up the duress PIN. Requires app password configured.
    SetupDuressPassword {
        duress_password: String,
    },
    /// Authenticate with a password. Returns Normal vs Duress mode.
    Authenticate {
        password: String,
    },
    /// Whether an app password is configured.
    IsPasswordEnabled,
    /// Whether duress mode is configured.
    IsDuressEnabled,
    /// Disable duress mode and clear duress hash/salt.
    DisableDuress,
    /// Configure the duress alert destination set + message.
    ConfigureDuressAlerts {
        contact_ids: Vec<String>,
        message: String,
    },
    /// Read the persisted duress alert settings.
    GetDuressSettings,
    /// Add a decoy contact (shown in duress mode). `card_json` is a
    /// JSON-serialised `ContactCard`. Returns the generated decoy id.
    AddDecoyContact {
        name: String,
        card_json: String,
    },
    /// List configured decoy contacts.
    ListDecoyContacts,
    /// Delete a decoy contact by id.
    DeleteDecoyContact {
        id: String,
    },
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
    GetDeliveryRecord {
        message_id: String,
    },
    /// All delivery records.
    GetAllDeliveryRecords,
    /// Delivery records for a specific recipient.
    GetDeliveryRecordsForContact {
        recipient_id: String,
    },
    /// Count of failed deliveries.
    CountFailedDeliveries,
    /// All failed delivery records.
    GetFailedDeliveryRecords,
    /// Reschedule a failed delivery for immediate retry.
    /// Returns `true` if the retry entry was found and rescheduled.
    ManualRetry {
        message_id: String,
    },
    /// All non-terminal pending deliveries.
    GetPendingDeliveries,
    /// Count of deliveries in a specific status.
    GetDeliveryCountByStatus {
        status: MobileDeliveryStatus,
    },
    /// All retry entries due for retry now.
    GetDueRetries,
    /// Retry entries for a specific contact.
    GetRetriesForContact {
        contact_id: String,
    },
    /// Total count of retry entries.
    GetRetryCount,
    /// Delete a retry entry by message id.
    DeleteRetry {
        message_id: String,
    },
    /// Compute the backoff (seconds) for a retry attempt. Pure.
    CalculateRetryBackoff {
        attempt: u32,
    },
    /// Total pending updates count (alias for `PendingUpdateCount`'s
    /// alternative implementation — counts via storage directly).
    GetTotalPendingCount,
    /// Whether the offline queue is at capacity.
    IsOfflineQueueFull,
    /// Remaining capacity in the offline queue.
    GetOfflineQueueCapacity,
    /// Drop all pending updates for a contact. Returns the cleared
    /// count.
    ClearPendingUpdatesForContact {
        contact_id: String,
    },
    /// Multi-device delivery summary for a message.
    GetDeliverySummary {
        message_id: String,
    },
    /// All device delivery records for a message.
    GetDeviceDeliveries {
        message_id: String,
    },
    /// All pending device deliveries.
    GetPendingDeviceDeliveries,
    // ── Identity reads + Onboarding helpers (B7 batch 9) ──
    /// Programmatically create an identity bypassing the onboarding
    /// `UserAction` flow. Errors when an identity already exists.
    CreateIdentity {
        display_name: String,
    },
    /// Read the active identity's public id (hex-encoded signing key).
    GetPublicId,
    /// Read the active identity's display name (own card).
    GetDisplayName,
    /// Read the active identity's signing-key fingerprint, formatted
    /// as 16 groups of 4 uppercase hex characters.
    GetOwnFingerprint,
    /// Compute display-name suggestions from a full name. Pure.
    DisplayNameSuggestions {
        full_name: String,
    },
    /// Reset the onboarding progress to step 0.
    ResetOnboarding,
    // ── Contact Verification + Duplicates + Notes + Misc (B7 batch 11) ──
    /// Mark a contact's fingerprint as verified.
    VerifyContact {
        id: String,
    },
    /// Mark a contact as trusted for simplified contact proposals
    /// (local-only flag).
    SetProposalTrusted {
        contact_id: String,
        trusted: bool,
    },
    /// Find duplicate-contact pairs.
    FindDuplicates,
    /// Dismiss a duplicate-contact pair so it stops being suggested.
    DismissDuplicate {
        id1: String,
        id2: String,
    },
    /// Save a personal note for a contact (cleared by passing "").
    SetContactNote {
        contact_id: String,
        note: String,
    },
    /// Read the personal note for a contact, if any.
    GetContactNote {
        contact_id: String,
    },
    /// Delete the personal note for a contact.
    DeleteContactNote {
        contact_id: String,
    },
    /// Save a private note on a specific field of a contact.
    SetContactFieldNote {
        contact_id: String,
        field_id: String,
        note: String,
    },
    /// Read all private field notes for a contact (sorted by
    /// `field_id` for deterministic output).
    GetContactFieldNotes {
        contact_id: String,
    },
    /// Delete the private note on a specific field of a contact.
    DeleteContactFieldNote {
        contact_id: String,
        field_id: String,
    },
    /// Set a local nickname for a contact.
    SetContactNickname {
        contact_id: String,
        name: String,
    },
    /// Clear the local nickname for a contact.
    ClearContactNickname {
        contact_id: String,
    },
    /// Set a custom avatar for a contact (must be WebP, ≤ 32 KB).
    SetContactCustomAvatar {
        contact_id: String,
        data: Vec<u8>,
    },
    /// Clear the custom avatar for a contact.
    ClearContactCustomAvatar {
        contact_id: String,
    },
    /// Get the custom avatar for a contact, if set.
    GetContactCustomAvatar {
        contact_id: String,
    },
    /// Search the social-network registry by query.
    SearchSocialNetworks {
        query: String,
    },
    /// Format a profile URL for a given social network and username.
    GetProfileUrl {
        network_id: String,
        username: String,
    },
    /// List hidden contacts (enriched).
    ListHiddenContacts,
    /// Returns the footer-button `ScreenAction` id that
    /// `ContactDetailEngine` would emit for the given contact.
    ContactDetailFooterActionId {
        contact_id: String,
    },

    // ── Backup + Import (B7 batch 12) ──
    /// Encrypt + export the active identity (legacy v1 backup).
    /// Returns base64-encoded backup data.
    ExportBackup {
        password: String,
    },
    /// Import an identity-only backup. Engine must have no active
    /// identity. `backup_data` is base64-encoded from `ExportBackup`.
    ImportBackup {
        backup_data: String,
        password: String,
    },
    /// Encrypt + export full v3 backup (identity + contacts + own
    /// card + labels). Returns base64-encoded data.
    ExportFullBackup {
        password: String,
    },
    /// Import a full v3 backup. Engine must have no active identity.
    /// `backup_data` is base64-encoded from `ExportFullBackup`.
    ImportFullBackup {
        backup_data: String,
        password: String,
    },
    /// Import contacts from a vCard 2.1/3.0/4.0 file. `data` is the
    /// raw `.vcf` bytes. Duplicates (by UID) are skipped.
    ImportContactsFromVcf {
        data: Vec<u8>,
    },
    // ── Offline Queue + Counts (B7 batch 13) ──
    // PendingUpdateCount, CountFailedDeliveries, GetTotalPendingCount,
    // IsOfflineQueueFull, GetOfflineQueueCapacity were already added by
    // batch 8. AddDecoyContact, ListDecoyContacts, DeleteDecoyContact
    // were added by batch 7. No new variants in batch 13.

    // ── Search + Display Prefs + Merge (B7 batch 14) ──
    // SearchContacts already declared by batch 10.
    /// Set the display-name preference for a contact. `pref_json`
    /// is a JSON-serialized `vauchi_core::DisplayNamePreference`
    /// (`"primary"`, `{"shared_name":{"name":"Alice"}}`, `"custom"`).
    SetDisplayNamePreference {
        contact_id: String,
        pref_json: String,
    },
    /// Set the avatar preference for a contact. `pref_json` is a
    /// JSON-serialized `vauchi_core::AvatarPreference`.
    SetAvatarPreference {
        contact_id: String,
        pref_json: String,
    },
    /// Merge `secondary_id` into `primary_id`. The secondary
    /// contact's unique fields are merged into the primary; the
    /// secondary is removed from storage. Returns the enriched
    /// merged contact.
    MergeContacts {
        primary_id: String,
        secondary_id: String,
    },
    // ── Field Visibility (B7 batch 15) ──
    // HideFieldFromContact, ShowFieldToContact, IsFieldVisibleToContact,
    // SetContactFieldOverride, RemoveContactFieldOverride were already
    // added by batch 6 (Field Visibility section).

    // ── Onboarding state ops (B7 batch 16) ──
    /// Read the current onboarding progress (step + completed steps).
    GetOnboardingProgress,
    /// Read the current onboarding step.
    CurrentOnboardingStep,
    /// Read whether onboarding has been completed end-to-end.
    IsOnboardingComplete,
    /// Mark the current step as completed and advance. Returns the
    /// updated progress.
    AdvanceOnboarding,
    /// Mark the current step as skipped and advance. Returns the
    /// updated progress.
    SkipOnboardingStep,

    // ── Contact display options + paginated lists (B7 batch 17) ──
    /// Read all display options (names + avatars) for a contact, with
    /// the active preference highlighted. Used by the chooser screen.
    GetContactDisplayOptions {
        contact_id: String,
    },
    /// List contacts with offset+limit pagination. Both bounds are
    /// applied at the storage layer; output is enriched.
    ListContactsPaginated {
        offset: u32,
        limit: u32,
    },
    // ListArchivedContacts is already declared in batch 10.

    // ── Sync flag persistence (B7 batch 18) ──
    /// Whether delivery-receipt ACKs (`ReceivedByRecipient`) are
    /// enabled. Persisted to a JSON sidecar file next to the storage
    /// directory.
    IsDeliveryReceiptsEnabled,
    /// Set the delivery-receipts flag. Persisted across restarts.
    SetDeliveryReceiptsEnabled {
        enabled: bool,
    },
    /// Whether presence suppression is enabled (the relay never
    /// learns whether the user is online). Persisted.
    IsSuppressPresenceEnabled,
    /// Set the suppress-presence flag. Persisted across restarts.
    SetSuppressPresenceEnabled {
        enabled: bool,
    },

    // ── Contact detail view state + social registry (B7 batch 19) ──
    /// Pre-computed contact-detail view (badges, banners, actions,
    /// added-time-display) — frontends iterate the returned arrays
    /// rather than branching on raw `MobileContact` flags. Closes
    /// ADR-021/043 audit V4.
    ContactDetailViewState {
        contact_id: String,
    },
    /// All social networks in the default registry. Complement to
    /// `SearchSocialNetworks`.
    ListSocialNetworks,

    // ── Multipart QR encoding (B7 batch 20) ──
    /// Encode arbitrary bytes into a sequence of multipart-QR
    /// payload strings (max ~1800 bytes per frame). Stateless —
    /// the decode side is a separate `MultipartDecoder`.
    EncodeMultipartQr {
        data: Vec<u8>,
    },

    // ── Certificate pinning (B7 batch 21) ──
    /// Set the pinned TLS certificate (PEM-encoded). Empty string
    /// disables pinning. Persisted to a sidecar file at `.cert_pin`
    /// so the choice survives restarts. Mirrors the legacy
    /// `VauchiPlatform::set_pinned_certificate`.
    SetPinnedCertificate {
        cert_pem: String,
    },
    /// Read whether certificate pinning is currently enabled.
    IsCertificatePinningEnabled,

    // ── Device linking — Track B Tier 2 retirement (B7 batch 22) ──
    //
    // Mirrors the typed device-linking methods previously on
    // `PlatformAppEngine`. Driven by the
    // `2026-05-23-track-b-push-to-zero-plan.md` campaign;
    // each method retires one `platform_app_engine_non_humble`
    // ratchet entry.
    /// Returns whether the current device is the primary device
    /// (`device_index == 0`). Mirrors the legacy
    /// `PlatformAppEngine::is_primary_device`.
    IsPrimaryDevice,
    /// Number of devices linked to the active identity. Returns 1
    /// when no on-disk `DeviceRegistry` exists yet (only the current
    /// device). Mirrors the legacy `PlatformAppEngine::device_count`.
    GetDeviceCount,
    /// List devices linked to the active identity (index 0 is the
    /// primary). Mirrors the legacy `PlatformAppEngine::get_devices`.
    GetDevices,
    /// Revoke the device at `device_index`. Mirrors the legacy
    /// `PlatformAppEngine::unlink_device`.
    UnlinkDevice {
        device_index: u32,
    },
    /// Generate the QR shown to a peer for device linking. Mirrors
    /// `PlatformAppEngine::generate_device_link_qr`.
    GenerateDeviceLinkQr,
    ParseDeviceLinkQr {
        qr_data: String,
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
    Bool {
        value: bool,
    },
    /// Aggregated `MobileConsentStatus` (B7 batch 1).
    ConsentStatus {
        status: MobileConsentStatus,
    },
    /// List of `MobileConsentRecord` (B7 batch 1).
    ConsentRecords {
        records: Vec<MobileConsentRecord>,
    },
    /// Outcome of `CheckContentUpdates` (B7 batch 2).
    UpdateStatus {
        status: MobileUpdateStatus,
    },
    /// Outcome of `ApplyContentUpdates` (B7 batch 2).
    ApplyResult {
        result: MobileApplyResult,
    },
    /// List of `MobileSocialNetwork` (B7 batch 2 — `ReloadSocialNetworks`).
    SocialNetworks {
        networks: Vec<MobileSocialNetwork>,
    },
    /// Numeric `u32` result — used by both B7 batch 3
    /// (`ExecuteIdentityDeletion` revocation count) and B7 batch 5
    /// (`AhaMomentsSeenCount`, `AhaMomentsTotalCount`).
    Count {
        value: u32,
    },
    /// List of devices linked to the active identity (B7 batch 22
    /// — `GetDevices`).
    Devices {
        devices: Vec<MobileDeviceInfo>,
    },
    /// Device-link QR payload (B7 batch 22 — `GenerateDeviceLinkQr`).
    DeviceLinkData {
        data: MobileDeviceLinkData,
    },
    /// Parsed device-link QR info (B7 b22).
    DeviceLinkInfo {
        info: MobileDeviceLinkInfo,
    },
    /// GDPR export payload (B7 batch 3 — `ExportGdprData`).
    GdprExport {
        export: MobileGdprExport,
    },
    /// Deletion-state snapshot (B7 batch 3 — `ScheduleIdentityDeletion`,
    /// `GetDeletionState`).
    DeletionInfo {
        info: MobileDeletionInfo,
    },
    /// Shred-process status snapshot (B7 batch 3 — `ShredStatus`).
    ShredStatus {
        status: MobileShredStatus,
    },
    /// Soft-shred scheduled — carries the token authorising a later
    /// hard-shred (B7 keychain batch — `SoftShred`).
    ShredScheduled {
        token: MobileShredToken,
    },
    /// Irreversible shred completed — per-step destruction report
    /// (B7 keychain batch — `HardShred` / `PanicShred`).
    ShredCompleted {
        report: MobileShredReport,
    },
    /// Optional aha-moment payload (B7 batch 5 —
    /// `TryTriggerAhaMoment` and friends).
    AhaMomentOpt {
        moment: Option<MobileAhaMoment>,
    },
    /// Optional demo-contact payload (B7 batch 5 —
    /// `InitDemoContactIfNeeded`, `GetDemoContact`,
    /// `TriggerDemoUpdate`, `RestoreDemoContact`).
    DemoContactOpt {
        contact: Option<MobileDemoContact>,
    },
    /// Demo-contact tracker state snapshot (B7 batch 5 —
    /// `GetDemoContactState`).
    DemoContactState {
        state: MobileDemoContactState,
    },
    /// Own contact card (B7 batch 10 — `GetOwnCard`).
    ContactCardPayload {
        card: MobileContactCard,
    },
    /// Optional contact (B7 batch 10 — `GetContact`).
    ContactOpt {
        contact: Option<MobileContact>,
    },
    /// List of contacts (B7 batch 10 — `ListContacts`,
    /// `SearchContacts`, `ListArchivedContacts`).
    Contacts {
        contacts: Vec<MobileContact>,
    },
    /// Recovery-proof verification result (B7 batch 4 —
    /// `VerifyRecoveryProof`).
    RecoveryVerification {
        verification: MobileRecoveryVerification,
    },
    RecoveryClaim {
        claim: MobileRecoveryClaim,
    },
    OptionalRecoveryProgress {
        progress: Option<MobileRecoveryProgress>,
    },
    RecoveryVoucher {
        voucher: MobileRecoveryVoucher,
    },
    RecoveryProgress {
        progress: MobileRecoveryProgress,
    },
    BroadcastResult {
        result: MobileBroadcastResult,
    },
    OptionalEmergencyConfig {
        config: Option<MobileEmergencyConfig>,
    },
    /// List of visibility labels (B7 batch 6 — `ListLabels`,
    /// `GetGroupsForContact`).
    Labels {
        labels: Vec<MobileVisibilityLabel>,
    },
    /// Single visibility label (B7 batch 6 — `CreateLabel`).
    Label {
        label: MobileVisibilityLabel,
    },
    /// Visibility label with resolved contact rows (B7 batch 6 —
    /// `GetLabel`).
    LabelDetail {
        detail: MobileVisibilityLabelDetail,
    },
    /// List of `String` payload (B7 batch 6 — `GetSuggestedLabels`).
    Strings {
        values: Vec<String>,
    },
    /// Generic `String` payload (B7 batch 7 — `AddDecoyContact`
    /// returns the generated decoy id).
    Text {
        value: String,
    },
    /// Authentication-mode result (B7 batch 7 — `Authenticate`).
    AuthMode {
        mode: MobileAuthMode,
    },
    /// Optional duress-settings payload (B7 batch 7 —
    /// `GetDuressSettings`).
    DuressSettingsOpt {
        settings: Option<MobileDuressSettings>,
    },
    /// List of decoy contacts (B7 batch 7 — `ListDecoyContacts`).
    DecoyContacts {
        contacts: Vec<MobileDecoyContact>,
    },
    /// Numeric u64 result (B7 batch 8 — `CalculateRetryBackoff`).
    BackoffSeconds {
        seconds: u64,
    },
    /// Optional delivery record (B7 batch 8 — `GetDeliveryRecord`).
    DeliveryRecordOpt {
        record: Option<MobileDeliveryRecord>,
    },
    /// List of delivery records (B7 batch 8 — multiple).
    DeliveryRecords {
        records: Vec<MobileDeliveryRecord>,
    },
    /// List of retry entries (B7 batch 8).
    RetryEntries {
        entries: Vec<MobileRetryEntry>,
    },
    /// Multi-device delivery summary (B7 batch 8 — `GetDeliverySummary`).
    DeliverySummary {
        summary: MobileDeliverySummary,
    },
    /// List of device delivery records (B7 batch 8 —
    /// `GetDeviceDeliveries`, `GetPendingDeviceDeliveries`).
    DeviceDeliveries {
        records: Vec<MobileDeviceDeliveryRecord>,
    },
    /// Optional `String` payload (B7 batch 11 — `GetContactNote`,
    /// `GetProfileUrl`).
    StringOpt {
        value: Option<String>,
    },
    /// Optional avatar bytes (B7 batch 11 — `GetContactCustomAvatar`).
    AvatarOpt {
        data: Option<Vec<u8>>,
    },
    /// List of duplicate-contact pairs (B7 batch 11 — `FindDuplicates`).
    DuplicatePairs {
        pairs: Vec<MobileDuplicatePair>,
    },
    /// List of field-notes (B7 batch 11 — `GetContactFieldNotes`).
    FieldNotes {
        notes: Vec<MobileFieldNote>,
    },
    /// vCard import outcome (B7 batch 12 — `ImportContactsFromVcf`).
    ImportResult {
        result: MobileImportResult,
    },
    /// Single enriched contact (B7 batch 14 — `MergeContacts`).
    ContactSingle {
        contact: MobileContact,
    },
    /// Onboarding progress snapshot (B7 batch 16 —
    /// `GetOnboardingProgress`, `AdvanceOnboarding`,
    /// `SkipOnboardingStep`).
    OnboardingProgress {
        progress: MobileOnboardingProgress,
    },
    /// Current onboarding step (B7 batch 16 — `CurrentOnboardingStep`).
    OnboardingStep {
        step: MobileOnboardingStep,
    },
    /// Contact display options snapshot (B7 batch 17 —
    /// `GetContactDisplayOptions`).
    ContactDisplayOptions {
        options: MobileContactDisplayOptions,
    },
    /// Pre-computed contact-detail view state (B7 batch 19 —
    /// `ContactDetailViewState`).
    ContactDetailView {
        state: MobileContactDetailViewState,
    },
}

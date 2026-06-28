// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device, sync, delivery, and retry types.
//!
//! Sync status/results, device linking, delivery tracking, retry queue,
//! and multi-device delivery types.

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSyncStatus {
    Idle,
    Syncing,
    Error,
}

/// Logical state of the sync-status *indicator*, as the frontend
/// observes it.
///
/// Distinct from [`MobileSyncStatus`] (the engine-level status):
/// the indicator also distinguishes "has synced before" (`Synced`)
/// from "never synced" (`NeverSynced`) to pick its label, while the
/// timestamp itself stays a UI-local concern (the frontend formats
/// the local clock into the `sync.synced_at` placeholder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSyncIndicatorState {
    Syncing,
    Error,
    Synced,
    NeverSynced,
}

/// Semantic color role for the sync-status label.
///
/// Frontends map this to their theme (e.g. Material
/// `colorScheme.primary` / `error` / `outline`) instead of hardcoding
/// a color per state (ADR-038/043 Humble UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSyncStatusKind {
    /// In-progress or freshly-synced — emphasis color.
    Active,
    /// Sync failed — error color.
    Error,
    /// Idle / never synced — muted color.
    Neutral,
}

/// Presentational view-state for the sync-status indicator.
///
/// Core owns the indicator-state → label/kind mapping so frontends
/// render `label_key` (via their i18n table) and map `kind` to a theme
/// color, instead of a per-platform `when (SyncState)` switch that
/// hardcodes both (ADR-021/043 Humble UI).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MobileSyncStatusView {
    /// i18n key for the status label (e.g. `"sync.synced_at"`, which may
    /// contain a `{time}` placeholder the frontend fills from the local
    /// clock).
    pub label_key: String,
    /// Semantic color role for the label.
    pub kind: MobileSyncStatusKind,
}

/// Compute the presentational view-state for the sync-status indicator.
///
/// Pure mapping — the single source of truth for which i18n label and
/// color role each indicator state shows.
#[uniffi::export]
pub fn sync_status_view(state: MobileSyncIndicatorState) -> MobileSyncStatusView {
    let (label_key, kind) = match state {
        MobileSyncIndicatorState::Syncing => ("sync.syncing", MobileSyncStatusKind::Active),
        MobileSyncIndicatorState::Error => ("sync.error_failed", MobileSyncStatusKind::Error),
        MobileSyncIndicatorState::Synced => ("sync.synced_at", MobileSyncStatusKind::Active),
        MobileSyncIndicatorState::NeverSynced => {
            ("sync.tap_to_sync", MobileSyncStatusKind::Neutral)
        }
    };
    MobileSyncStatusView {
        label_key: label_key.to_string(),
        kind,
    }
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
    /// Total number of operations (contacts_added + cards_updated + updates_sent).
    pub total: u32,
    /// Whether any changes were synced.
    pub has_changes: bool,
    /// Display names of contacts whose cards were updated (for UI notification).
    pub updated_contact_names: Vec<String>,
    /// Diagnostics: total blobs fetched from the mailbox this sync.
    /// `blobs_fetched=0` = nothing delivered (relay store/forward or token);
    /// `>0` with `cards_updated=0` = arrived but rejected (decrypt).
    /// 2026-06-28-sync-delivery-sent-not-received.
    pub blobs_fetched: u32,
    /// Diagnostics: token-resolved but undecryptable.
    pub rejected: u32,
    /// Diagnostics: token-unresolved (no contact-token match).
    pub unresolved: u32,
}

/// Maps a core sync outcome to the mobile result shape.
///
/// Throttle decision (engine-resident-sync-orchestration design §4):
/// `TooSoon` is a benign no-change result, *not* an error — a
/// user-initiated sync inside the C1/C2 privacy window reports
/// "up to date" rather than failing. `NotConnected` / `NoIdentity`
/// stay errors so the caller distinguishes them from a successful
/// empty sync. The `Ok` field mapping mirrors the retired
/// `VauchiPlatform::sync()` exactly (`received → cards_updated`,
/// `sent → updates_sent`; `contacts_added` / `updated_contact_names`
/// have no source in the outcome).
impl TryFrom<vauchi_core::api::VauchiSyncOutcome> for MobileSyncResult {
    type Error = crate::error::MobileError;

    fn try_from(outcome: vauchi_core::api::VauchiSyncOutcome) -> Result<Self, Self::Error> {
        use vauchi_core::api::VauchiSyncOutcome;
        match outcome {
            VauchiSyncOutcome::Ok {
                received,
                fetched,
                rejected,
                unresolved,
                sent,
                ..
            } => Ok(MobileSyncResult {
                contacts_added: 0,
                cards_updated: received as u32,
                updates_sent: sent as u32,
                total: (received + sent) as u32,
                has_changes: received > 0 || sent > 0,
                updated_contact_names: vec![],
                blobs_fetched: fetched as u32,
                rejected: rejected as u32,
                unresolved: unresolved as u32,
            }),
            VauchiSyncOutcome::TooSoon => Ok(MobileSyncResult {
                contacts_added: 0,
                cards_updated: 0,
                updates_sent: 0,
                total: 0,
                has_changes: false,
                updated_contact_names: vec![],
                blobs_fetched: 0,
                rejected: 0,
                unresolved: 0,
            }),
            VauchiSyncOutcome::NotConnected => Err(crate::error::MobileError::Other {
                detail: "Not connected".into(),
            }),
            VauchiSyncOutcome::NoIdentity => Err(crate::error::MobileError::Other {
                detail: "No identity".into(),
            }),
        }
    }
}

/// Incoming device link request received via relay.
///
/// Returned by `listen_for_device_link_request` on the existing device.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceLinkRequest {
    /// Encrypted device link request payload.
    pub encrypted_payload: Vec<u8>,
    /// Sender token for routing the response back.
    pub sender_token: String,
}

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

/// Confirmation details for device link (shown before approving).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceLinkConfirmation {
    /// The new device's proposed name.
    pub device_name: String,
    /// 6-digit confirmation code (formatted as `XXX-XXX`).
    pub confirmation_code: String,
    /// Identity fingerprint (e.g. `AB12-CD34-EF56-7890`).
    pub identity_fingerprint: String,
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
    /// Encrypted response bytes for the new device (base64-encoded).
    pub encrypted_response: Option<Vec<u8>>,
}

/// Result of joining a device link (for new device).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceJoinResult {
    /// Whether joining was successful.
    pub success: bool,
    /// Display name of the identity.
    pub display_name: String,
    /// Assigned device index.
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
            _ => MobileDeliveryStatus::Queued,
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
            _ => MobileDeviceDeliveryStatus::Pending,
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

// INLINE_TEST_REQUIRED: sync_status_view is a pure mapping; co-locate its
// label-key/kind table with the implementation.
#[cfg(test)]
mod sync_status_view_tests {
    use super::{MobileSyncIndicatorState, MobileSyncStatusKind, sync_status_view};

    // @internal
    #[test]
    fn maps_each_indicator_state_to_label_and_kind() {
        let cases = [
            (
                MobileSyncIndicatorState::Syncing,
                "sync.syncing",
                MobileSyncStatusKind::Active,
            ),
            (
                MobileSyncIndicatorState::Error,
                "sync.error_failed",
                MobileSyncStatusKind::Error,
            ),
            (
                MobileSyncIndicatorState::Synced,
                "sync.synced_at",
                MobileSyncStatusKind::Active,
            ),
            (
                MobileSyncIndicatorState::NeverSynced,
                "sync.tap_to_sync",
                MobileSyncStatusKind::Neutral,
            ),
        ];
        for (state, expected_key, expected_kind) in cases {
            let view = sync_status_view(state);
            assert_eq!(view.label_key, expected_key, "label for {state:?}");
            assert_eq!(view.kind, expected_kind, "kind for {state:?}");
        }
    }
}

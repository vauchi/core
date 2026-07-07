// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Security, privacy, and compliance types.
//!
//! Recovery, GDPR/deletion, consent, shred, duress, and emergency broadcast.

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

/// Deletion state for mobile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileDeletionState {
    /// No deletion scheduled.
    None,
    /// Deletion scheduled with grace period.
    Scheduled,
    /// Deletion has been executed.
    Executed,
}

impl From<&vauchi_core::storage::DeletionState> for MobileDeletionState {
    fn from(state: &vauchi_core::storage::DeletionState) -> Self {
        match state {
            vauchi_core::storage::DeletionState::None => MobileDeletionState::None,
            vauchi_core::storage::DeletionState::Scheduled { .. } => MobileDeletionState::Scheduled,
            vauchi_core::storage::DeletionState::Executed { .. } => MobileDeletionState::Executed,
            _ => MobileDeletionState::None,
        }
    }
}

/// Deletion info with timing details.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeletionInfo {
    /// Current deletion state.
    pub state: MobileDeletionState,
    /// When deletion was scheduled (0 if not scheduled).
    pub scheduled_at: u64,
    /// When deletion can be executed (0 if not scheduled).
    pub execute_at: u64,
    /// Days remaining in grace period (0 if not scheduled).
    pub days_remaining: u32,
}

impl From<&vauchi_core::storage::DeletionState> for MobileDeletionInfo {
    fn from(state: &vauchi_core::storage::DeletionState) -> Self {
        match state {
            vauchi_core::storage::DeletionState::None => MobileDeletionInfo {
                state: MobileDeletionState::None,
                scheduled_at: 0,
                execute_at: 0,
                days_remaining: 0,
            },
            vauchi_core::storage::DeletionState::Scheduled {
                scheduled_at,
                execute_at,
            } => MobileDeletionInfo {
                state: MobileDeletionState::Scheduled,
                scheduled_at: *scheduled_at,
                execute_at: *execute_at,
                days_remaining: 0,
            },

            vauchi_core::storage::DeletionState::Executed { .. } => MobileDeletionInfo {
                state: MobileDeletionState::Executed,
                scheduled_at: 0,
                execute_at: 0,
                days_remaining: 0,
            },
            _ => MobileDeletionInfo {
                state: MobileDeletionState::None,
                scheduled_at: 0,
                execute_at: 0,
                days_remaining: 0,
            },
        }
    }
}

/// GDPR data export result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileGdprExport {
    /// Exported data as JSON string.
    pub json_data: String,
    /// When the export was created (Unix timestamp).
    pub exported_at: u64,
    /// Export format version.
    pub version: u32,
}

/// Types of consent that can be granted or revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileConsentType {
    /// Consent for local data processing.
    DataProcessing,
    /// Consent for sharing contact information.
    ContactSharing,
    /// Consent to participate in recovery vouching.
    RecoveryVouching,
}

impl From<MobileConsentType> for vauchi_core::api::ConsentType {
    fn from(ct: MobileConsentType) -> Self {
        match ct {
            MobileConsentType::DataProcessing => vauchi_core::api::ConsentType::DataProcessing,
            MobileConsentType::ContactSharing => vauchi_core::api::ConsentType::ContactSharing,
            MobileConsentType::RecoveryVouching => vauchi_core::api::ConsentType::RecoveryVouching,
        }
    }
}

impl From<&vauchi_core::api::ConsentType> for MobileConsentType {
    fn from(ct: &vauchi_core::api::ConsentType) -> Self {
        match ct {
            vauchi_core::api::ConsentType::DataProcessing => MobileConsentType::DataProcessing,
            vauchi_core::api::ConsentType::ContactSharing => MobileConsentType::ContactSharing,
            vauchi_core::api::ConsentType::RecoveryVouching => MobileConsentType::RecoveryVouching,
            _ => MobileConsentType::DataProcessing,
        }
    }
}

/// A recorded consent decision.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileConsentRecord {
    /// Unique record ID.
    pub id: String,
    /// Type of consent.
    pub consent_type: MobileConsentType,
    /// Whether consent was granted.
    pub granted: bool,
    /// Unix timestamp of the decision.
    pub timestamp: u64,
    /// Privacy policy version at time of consent.
    pub policy_version: Option<String>,
}

impl From<&vauchi_core::api::ConsentRecord> for MobileConsentRecord {
    fn from(record: &vauchi_core::api::ConsentRecord) -> Self {
        MobileConsentRecord {
            id: record.id.clone(),
            consent_type: MobileConsentType::from(&record.consent_type),
            granted: record.granted,
            timestamp: record.timestamp,
            policy_version: record.policy_version.clone(),
        }
    }
}

/// Aggregated consent status for a specific consent type.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileConsentStatus {
    /// Whether consent is currently granted.
    pub granted: bool,
    /// Unix timestamp of the most recent grant or revocation, if any.
    pub last_changed_at: Option<u64>,
    /// Privacy policy version from the most recent consent record, if any.
    pub policy_version: Option<String>,
}

impl From<vauchi_core::api::ConsentStatus> for MobileConsentStatus {
    fn from(status: vauchi_core::api::ConsentStatus) -> Self {
        MobileConsentStatus {
            granted: status.granted,
            last_changed_at: status.last_changed_at,
            policy_version: status.policy_version,
        }
    }
}

/// Token returned by soft_shred to authorize hard_shred.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileShredToken {
    /// When the token was created (unix seconds).
    pub created_at: u64,
}

impl From<&vauchi_core::api::ShredToken> for MobileShredToken {
    fn from(token: &vauchi_core::api::ShredToken) -> Self {
        MobileShredToken {
            created_at: token.created_at(),
        }
    }
}

/// Report of shred operations performed.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileShredReport {
    /// Number of contacts notified of deletion.
    pub contacts_notified: u32,
    /// Whether the relay purge was sent successfully.
    pub relay_purge_sent: bool,
    /// Number of linked devices notified.
    pub devices_notified: u32,
    /// Whether SMK was destroyed from SecureStorage.
    pub smk_destroyed: bool,
    /// Whether the identity backup file was securely deleted.
    pub identity_file_destroyed: bool,
    /// Number of key files deleted.
    pub key_files_destroyed: u32,
    /// Whether the SQLite database was securely deleted.
    pub sqlite_destroyed: bool,
    /// Whether the pre-signed messages file was deleted.
    pub pre_signed_deleted: bool,
    /// Whether the data directory was removed.
    pub data_dir_deleted: bool,
    /// Whether purge sender construction failed.
    pub purge_failed: bool,
    /// Error message if purge sender failed to construct.
    pub purge_error: Option<String>,
    /// Whether revocation sender construction failed.
    pub revocation_failed: bool,
    /// Error message if revocation sender failed to construct.
    pub revocation_error: Option<String>,
}

impl From<&vauchi_core::api::ShredReport> for MobileShredReport {
    fn from(report: &vauchi_core::api::ShredReport) -> Self {
        MobileShredReport {
            contacts_notified: report.contacts_notified as u32,
            relay_purge_sent: report.relay_purge_sent,
            devices_notified: report.devices_notified as u32,
            smk_destroyed: report.smk_destroyed,
            identity_file_destroyed: report.identity_file_destroyed,
            key_files_destroyed: report.key_files_destroyed as u32,
            sqlite_destroyed: report.sqlite_destroyed,
            pre_signed_deleted: report.pre_signed_deleted,
            data_dir_deleted: report.data_dir_deleted,
            purge_failed: false,
            purge_error: None,
            revocation_failed: false,
            revocation_error: None,
        }
    }
}

/// Current shred status for the identity.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileShredStatus {
    /// No shred operation in progress.
    None,
    /// Soft shred scheduled — waiting for grace period to elapse.
    Scheduled {
        /// Seconds remaining in grace period.
        remaining_secs: u64,
    },
    /// Hard shred has been executed — all data destroyed.
    Executed,
}

// =============================================================================
// =============================================================================
// Duress PIN Types
// =============================================================================

/// Authentication mode result for mobile platforms.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileAuthMode {
    /// The normal (real) password was used.
    Normal,
    /// The duress PIN was used — show decoy contacts only.
    Duress,
}

/// Outcome of `PlatformAppEngine.biometric_unlock_check()`.
///
/// Mirror of [`vauchi_core::BiometricUnlockOutcome`] crossing the
/// UniFFI boundary. The call wraps the duress-aware decision in a
/// constant-time floor so the unlock animation timing cannot leak
/// whether duress is configured (audit item P2-B in
/// `2026-04-28-lifecycle-session-residue-umbrella`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileBiometricUnlockOutcome {
    /// Biometric authentication succeeded and no duress PIN is
    /// configured — frontends transition to the post-auth screen.
    Unlocked,
    /// Biometric authentication succeeded but duress is configured —
    /// frontends must show the PIN entry screen so the user types
    /// either the real PIN (`Normal`) or the duress PIN (`Duress`).
    PromptForDuressPin,
}

impl From<vauchi_core::BiometricUnlockOutcome> for MobileBiometricUnlockOutcome {
    fn from(value: vauchi_core::BiometricUnlockOutcome) -> Self {
        match value {
            vauchi_core::BiometricUnlockOutcome::Unlocked => Self::Unlocked,
            vauchi_core::BiometricUnlockOutcome::PromptForDuressPin => Self::PromptForDuressPin,
            _ => Self::Unlocked,
        }
    }
}

/// Duress alert settings for mobile platforms.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDuressSettings {
    /// Contact IDs of trusted contacts who receive duress alerts.
    pub alert_contact_ids: Vec<String>,
    /// Custom alert message included in the alert payload.
    pub alert_message: String,
    /// Whether to include device location in the alert.
    pub include_location: bool,
}

/// A decoy contact for mobile platforms.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDecoyContact {
    /// Unique identifier for the decoy contact.
    pub id: String,
    /// Display name shown in the contact list.
    pub display_name: String,
}

/// Emergency broadcast result for mobile platforms.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileBroadcastResult {
    /// Number of alerts successfully queued for delivery.
    pub sent: u32,
    /// Total number of trusted contacts in the config.
    pub total: u32,
}

/// Emergency broadcast configuration for mobile platforms.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileEmergencyConfig {
    /// Contact IDs of trusted contacts who receive emergency alerts.
    pub trusted_contact_ids: Vec<String>,
    /// Custom alert message included in the alert payload.
    pub message: String,
    /// Whether to include device location in the alert.
    pub include_location: bool,
}

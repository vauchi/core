// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared domain types used across multiple modules.
//!
//! These types are used by exchange, contact, capability, and storage modules.
//! Placing them here avoids circular dependencies and prepares for future crate
//! extraction (vauchi-types).

/// Transport method used for contact exchange.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    Ble,
}

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProximityConfidence {
    /// High confidence: verified by ultrasonic audio or NFC tap.
    High,
    /// Medium confidence: manual user confirmation.
    Medium,
    /// Low confidence: proximity check failed or timed out.
    Low,
    /// Unknown: no proximity check was performed (legacy contacts).
    #[default]
    Unknown,
}

/// Represents device audio capabilities.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AudioCapability {
    /// Device supports full ultrasonic audio (speaker + microphone)
    Full,
    /// Device can only emit ultrasonic audio (no microphone)
    EmitOnly,
    /// Device can only receive ultrasonic audio (no speaker)
    ReceiveOnly,
    /// Device does not support ultrasonic audio
    #[default]
    None,
}

// --- API types used by storage (breaks storage→api circular dep) ---

/// Duress settings for emergency alert configuration.
///
/// Stored in the `duress_settings` table (migration V20).
/// Determines which contacts receive alerts, what message is included,
/// and whether device location is included.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuressSettings {
    /// Contact IDs of trusted contacts who receive duress alerts.
    pub alert_contact_ids: Vec<String>,
    /// Custom alert message included in the alert payload.
    pub alert_message: String,
    /// Whether to include device location in the alert.
    pub include_location: bool,
}

/// Emergency broadcast configuration.
///
/// Stored in the `emergency_config` table (migration V22).
/// Determines which contacts receive alerts, what message is sent,
/// and whether device location is included.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmergencyBroadcastConfig {
    /// Contact IDs of trusted contacts who receive emergency alerts.
    pub trusted_contact_ids: Vec<String>,
    /// Custom alert message included in the alert payload.
    pub message: String,
    /// Whether to include device location in the alert.
    pub include_location: bool,
}

// --- UX types used by storage and API ---

/// Steps in the onboarding wizard.
///
/// The user progresses through these in order, though backward
/// transitions are always allowed and some steps can be skipped.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, PartialOrd, Ord,
)]
pub enum OnboardingStep {
    /// Pre-gate: does the user already have an identity?
    IdentityCheck,
    /// Pre-gate: choose how to restore (link device or import backup)
    LinkChoice,
    /// Welcome screen showing value proposition
    Welcome,
    /// Default display name entry (renamed from CreateIdentity)
    #[serde(alias = "CreateIdentity")]
    DefaultName,
    /// Skip gate: user can skip to finish or continue setup
    SkipGate,
    /// Groups setup: create contact groups
    GroupsSetup,
    /// Contact info fields (phone, email) (renamed from AddFields)
    #[serde(alias = "AddFields")]
    ContactInfo,
    /// Preview the contact card before continuing
    PreviewCard,
    /// Security explanation screen
    SecurityExplanation,
    /// Prompt to set up backup
    BackupPrompt,
    /// Onboarding complete, ready to use
    Ready,
}

/// Tracks the user's progress through the onboarding wizard.
///
/// Follows the same persistence pattern as `DemoContactState` and
/// `AhaMomentTracker` — serialized to JSON, encrypted, and stored
/// in the `ux_state` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OnboardingProgress {
    /// The step the user is currently on.
    pub current_step: OnboardingStep,
    /// Steps that have been completed (visited and passed).
    pub completed_steps: std::collections::HashSet<OnboardingStep>,
    /// Timestamp when onboarding was started (Unix epoch seconds).
    pub started_at: Option<u64>,
    /// Timestamp when onboarding was completed (Unix epoch seconds).
    pub completed_at: Option<u64>,
    /// Whether the user skipped the backup step.
    pub skipped_backup: bool,
}

/// State of the demo contact.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DemoContactState {
    /// Whether the demo contact is active.
    pub is_active: bool,
    /// Whether it was manually dismissed.
    pub was_dismissed: bool,
    /// Whether it was auto-removed after first real exchange.
    pub auto_removed: bool,
    /// Current tip index (which tip is being shown).
    pub current_tip_index: usize,
    /// Timestamp of last update (Unix epoch seconds).
    pub last_update_timestamp: u64,
    /// History of shown tip IDs.
    pub shown_tip_ids: Vec<String>,
    /// Number of updates sent.
    pub update_count: u32,
}

// --- Tor types used by storage and network ---

/// Current status of the Tor connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TorStatus {
    /// Tor is not enabled.
    Disabled,
    /// Tor client is connecting to the network.
    Connecting,
    /// Tor client is bootstrapping (downloading directory info).
    Bootstrapping {
        /// Bootstrap progress percentage (0-100).
        percentage: u8,
    },
    /// Tor client is connected and ready.
    Connected,
    /// Tor client is disconnected.
    Disconnected {
        /// Reason for disconnection.
        reason: String,
    },
}

/// Configuration for Tor connectivity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorConfig {
    /// Whether Tor mode is enabled.
    pub enabled: bool,
    /// Bridge addresses for censored networks (obfs4 format).
    pub bridges: Vec<String>,
    /// Whether to prefer .onion addresses when available.
    pub prefer_onion: bool,
    /// How often to rotate Tor circuits (in seconds). Default: 600 (10 minutes).
    pub circuit_rotation_secs: u64,
}

/// A relay address that may have both clearnet and .onion URLs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TorRelayAddress {
    /// The clearnet URL (e.g. wss://relay.vauchi.app).
    pub clearnet_url: String,
    /// The optional .onion URL.
    pub onion_url: Option<String>,
}

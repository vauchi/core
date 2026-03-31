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
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    #[serde(alias = "Qr")]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Nfc")]
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Ble")]
    Ble,
    /// USB cable exchange: TCP over physical cable connection.
    Usb,
    /// Audio data channel exchange: ultrasonic or audible payload transfer.
    Audio,
}

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
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

impl AudioCapability {
    /// Returns true if any audio hardware is available.
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::None)
    }
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
#[non_exhaustive]
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

// --- Visibility types (breaks contact ↔ contact_card circular dep) ---

/// Visibility setting for a single field.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum FieldVisibility {
    /// Visible to everyone (default for new fields)
    #[default]
    Everyone,
    /// Visible only to specific contacts
    Contacts(std::collections::HashSet<String>),
    /// Visible to no one (private)
    Nobody,
}

/// Visibility rules for all fields in a contact card.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct VisibilityRules {
    /// Map from field ID to visibility setting.
    /// `pub(crate)` so the impl block in `contact::visibility` can access it.
    pub(crate) rules: std::collections::HashMap<String, FieldVisibility>,
}

// --- Aha moment types used by storage and API ---

/// Types of aha moments that can be triggered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum AhaMomentType {
    /// Shown when card creation completes
    CardCreationComplete,
    /// Shown on first edit (before having contacts)
    FirstEdit,
    /// Shown when first contact is added
    FirstContactAdded,
    /// Shown when receiving first update from a contact
    FirstUpdateReceived,
    /// Shown when first outbound update is delivered
    FirstOutboundDelivered,
    /// Shown when the user edits a field on their card for the first time
    FirstFieldEdit,
    /// Shown when the user reaches three contacts
    ThreeContactsReached,
    /// Shown when a second device is linked
    DeviceLinked,
}

/// Tracks which aha moments have been seen
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AhaMomentTracker {
    /// Set of seen moment types
    seen: std::collections::HashSet<AhaMomentType>,
}

impl AhaMomentTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a moment type has been seen
    pub fn has_seen(&self, moment_type: AhaMomentType) -> bool {
        self.seen.contains(&moment_type)
    }

    /// Mark a moment as seen
    pub fn mark_seen(&mut self, moment_type: AhaMomentType) {
        self.seen.insert(moment_type);
    }

    /// Check if a moment should be triggered (not yet seen)
    pub fn should_trigger(&self, moment_type: AhaMomentType) -> bool {
        !self.has_seen(moment_type)
    }

    /// Get count of seen moments
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Get count of total possible moments
    pub fn total_count(&self) -> usize {
        AhaMomentType::all().len()
    }

    /// Reset all seen moments (for testing/debugging)
    pub fn reset(&mut self) {
        self.seen.clear()
    }

    /// Serialize to JSON for storage
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl AhaMomentType {
    /// Get all aha moment types in order
    pub fn all() -> &'static [AhaMomentType] {
        &[
            AhaMomentType::CardCreationComplete,
            AhaMomentType::FirstEdit,
            AhaMomentType::FirstContactAdded,
            AhaMomentType::FirstUpdateReceived,
            AhaMomentType::FirstOutboundDelivered,
            AhaMomentType::FirstFieldEdit,
            AhaMomentType::ThreeContactsReached,
            AhaMomentType::DeviceLinked,
        ]
    }
}

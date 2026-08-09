// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency, duress, and biometric-unlock domain types.
//!
//! A neutral leaf module. These are shared by `storage` (persistence) and
//! the feature-gated `api` layer; keeping them here — always compiled and
//! depending on nothing — is what breaks the `storage → api` cycle they
//! would otherwise create.

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

/// The default emergency broadcast alert message.
pub const DEFAULT_EMERGENCY_MESSAGE: &str = "I may be in danger. Please check on me.";

/// Maximum number of trusted contacts for emergency broadcast.
pub const MAX_TRUSTED_CONTACTS: usize = 10;

/// The number of digits in a duress PIN.
///
/// Lives beside `DuressSettings` rather than in the API layer so the UI
/// reducer can reach it without pulling in a network feature. Keeping a
/// second copy in the reducer is how the two drifted: it capped typed
/// input at six while a pasted value bypassed the cap entirely and the
/// API accepted whatever arrived.
pub const DURESS_PIN_LENGTH: usize = 6;

impl EmergencyBroadcastConfig {
    /// Returns `true` if the alert message is the default (not customized).
    pub fn is_default_message(&self) -> bool {
        self.message == DEFAULT_EMERGENCY_MESSAGE
    }
}

/// Snapshot of emergency-related configuration and state.
///
/// Returned by [`Vauchi::get_emergency_wipe_status`](crate::api::Vauchi::get_emergency_wipe_status)
/// so frontends can render an emergency readiness overview without issuing
/// multiple API calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmergencyWipeStatus {
    /// An emergency broadcast has been configured with at least one trusted contact.
    pub broadcast_configured: bool,
    /// Duress settings have been configured.
    pub duress_configured: bool,
    /// Identity deletion is scheduled (grace period active).
    pub deletion_scheduled: bool,
    /// Identity deletion has already been executed.
    pub deletion_executed: bool,
    /// At least one contact is marked as recovery-trusted.
    pub has_trusted_contacts: bool,
    /// Number of recovery-trusted contacts.
    pub trusted_contact_count: usize,
    /// An app password has been configured.
    pub password_enabled: bool,
}

/// Outcome of [`crate::api::Vauchi::biometric_unlock_check`].
///
/// Returned after a successful platform biometric authentication
/// (LAContext on iOS, BiometricPrompt on Android). The variant tells
/// the frontend which screen to render next:
///
/// - `Unlocked`: biometric proves the real user; transition to the
///   post-auth screen.
/// - `PromptForDuressPin`: a duress PIN is configured, so the user
///   must enter the PIN — that PIN check determines `Normal` vs
///   `Duress` mode via [`crate::api::Vauchi::authenticate`].
///
/// The dispatcher is constant-time: the wall-clock duration of the
/// containing call is at least
/// [`crate::api::BIOMETRIC_UNLOCK_MIN_DURATION`] regardless of which
/// outcome is returned, so an observer cannot infer whether duress is
/// configured by timing the unlock animation.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BiometricUnlockOutcome {
    /// Biometric authentication succeeded and no duress PIN is
    /// configured — the user is fully unlocked. `auth_mode` is set to
    /// [`crate::api::AuthMode::Normal`].
    Unlocked,
    /// Biometric authentication succeeded but a duress PIN is
    /// configured — the frontend must present the PIN entry screen so
    /// the user enters either the real PIN or the duress PIN. The
    /// subsequent [`crate::api::Vauchi::authenticate`] call sets the
    /// final [`crate::api::AuthMode`].
    PromptForDuressPin,
}

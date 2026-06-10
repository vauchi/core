// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed engine↔`AppEngine` channels.
//!
//! `EngineOutput` (engine → hub) carries the salient state a workflow
//! engine exposes at completion or interception time. It replaces the
//! `as_any` downcast reads and the stringly-typed `collected_input`
//! channel: every payload that crosses the seam is a closed-enum
//! variant, so a wrong discriminator is a compile error, not a silent
//! runtime no-op.
//!
//! Mismatch policy: when the active engine is not the one a hub site
//! expects (overlay or lock engine active while a stale async result
//! lands), `engine_output()` yields `None` or a foreign variant — hub
//! sites `tracing::warn!` and degrade exactly as a failed downcast did.
//!
//! Record: `2026-06-10-appengine-typed-engine-channel`.

use super::emergency_broadcast::EmergencyOutcome;
use super::fingerprint_verify::VerifyAction;
use super::onboarding::OnboardingData;

/// Salient typed state an engine exposes to `AppEngine`.
///
/// One variant per engine that the hub reads; each variant is the
/// engine's full interception-relevant snapshot so a single
/// parameterless getter suffices.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineOutput {
    /// Outcome of the fingerprint-verification screen.
    FingerprintVerify(VerifyAction),
    /// Everything captured during onboarding (display name, group
    /// selection, ContactInfo fields), read at completion time.
    Onboarding(Box<OnboardingData>),
    /// The configured emergency broadcast and the user's chosen outcome.
    EmergencyBroadcast(EmergencyBroadcastPlan),
    /// The display name as edited on the contact-edit screen.
    ContactEdit { display_name: String },
    /// The backup/restore form state (password redacted in `Debug`).
    Backup(BackupFormSnapshot),
    /// PIN/password typed on the lock screen (redacted in `Debug`).
    /// Absent (engine returns `None`) while the entry is empty.
    Lock { pin: String },
    /// Per-field show/hide toggles on the contact-visibility screen,
    /// as `(field_id, visible)` pairs.
    ContactVisibility { toggles: Vec<(String, bool)> },
    /// The sync screen's pressed action.
    Sync(SyncChoice),
    /// The privacy/GDPR operation the user confirmed.
    Gdpr(GdprChoice),
    /// A form-dialog submission, typed per dialog kind — replaces the
    /// retired newline-packed `collected_input` strings.
    Form(FormInput),
}

/// Action pressed on the sync-status screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncChoice {
    SyncNow,
    TestConnection,
}

/// Operation confirmed on the privacy/GDPR screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdprChoice {
    Export,
    Delete,
    CancelDeletion,
    Execute,
    Shred,
}

/// Typed form-dialog submission, one variant per [`FormDialogType`].
///
/// [`FormDialogType`]: super::form_dialog::FormDialogType
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormInput {
    EditName {
        name: String,
    },
    EditField {
        value: String,
        note: String,
    },
    AddField {
        entry_type: String,
        label: String,
        value: String,
        note: String,
        groups: Vec<String>,
    },
    CreateGroup {
        name: String,
    },
    RenameGroup {
        name: String,
    },
    EditRelayUrl {
        url: String,
    },
}

/// Snapshot of the emergency-broadcast engine at completion time.
#[derive(Clone, Debug, PartialEq)]
pub struct EmergencyBroadcastPlan {
    pub outcome: Option<EmergencyOutcome>,
    pub contact_ids: Vec<String>,
    pub message: String,
    pub include_location: bool,
}

/// Snapshot of the backup/restore form.
///
/// `password` is a live credential: it must never reach logs, so
/// [`EngineOutput`]'s `Debug` is hand-written to redact it (hub
/// mismatch warns print the foreign variant with `?other`).
#[derive(Clone, PartialEq)]
pub struct BackupFormSnapshot {
    pub restore_mode: bool,
    pub restore_data: String,
    pub password: String,
    pub full_level: bool,
}

impl std::fmt::Debug for BackupFormSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupFormSnapshot")
            .field("restore_mode", &self.restore_mode)
            .field("restore_data_len", &self.restore_data.len())
            .field("password", &"<redacted>")
            .field("full_level", &self.full_level)
            .finish()
    }
}

impl std::fmt::Debug for EngineOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FingerprintVerify(a) => f.debug_tuple("FingerprintVerify").field(a).finish(),
            Self::Onboarding(d) => f.debug_tuple("Onboarding").field(d).finish(),
            Self::EmergencyBroadcast(p) => f.debug_tuple("EmergencyBroadcast").field(p).finish(),
            Self::ContactEdit { display_name } => f
                .debug_struct("ContactEdit")
                .field("display_name", display_name)
                .finish(),
            Self::Backup(s) => f.debug_tuple("Backup").field(s).finish(),
            Self::Lock { .. } => f.debug_struct("Lock").field("pin", &"<redacted>").finish(),
            Self::ContactVisibility { toggles } => f
                .debug_struct("ContactVisibility")
                .field("toggles", toggles)
                .finish(),
            Self::Sync(c) => f.debug_tuple("Sync").field(c).finish(),
            Self::Gdpr(c) => f.debug_tuple("Gdpr").field(c).finish(),
            Self::Form(i) => f.debug_tuple("Form").field(i).finish(),
        }
    }
}

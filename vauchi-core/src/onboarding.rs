// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding State Machine
//!
//! Tracks user progress through the first-run onboarding wizard.
//! Persisted as encrypted UX state; exposed via `Vauchi` API and UniFFI.
//!
//! Feature file: features/onboarding.feature

use std::collections::HashSet;

use crate::text::normalize_text;

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
    /// Pre-gate: instructions for linking this fresh device to an existing
    /// identity (scan the QR code or open the invitation link from the
    /// other device). Reached from `IdentityCheck` via `link_device`.
    DeviceLinkInstructions,
    /// Default display name entry (renamed from CreateIdentity)
    #[serde(alias = "CreateIdentity", alias = "Welcome", alias = "SkipGate")]
    DefaultName,
    /// Groups setup: create contact groups
    GroupsSetup,
    /// Contact info fields (phone, email) (renamed from AddFields)
    #[serde(alias = "AddFields", alias = "PreviewCard")]
    ContactInfo,
    /// Choose what to do after onboarding
    #[serde(alias = "SecurityExplanation", alias = "BackupPrompt", alias = "Ready")]
    WhatNext,
    /// Password entry for backup restore (after the user has picked the
    /// encrypted backup file via [`crate::exchange::Command::
    /// FilePickFromUser`] with `purpose = FilePickPurpose::ImportBackup`).
    /// Submitting calls [`crate::api::Vauchi::import_full_backup`] in
    /// the AppEngine completion path; success creates identity from
    /// the restored data and routes to MainScreen.
    BackupPasswordEntry,
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

impl OnboardingStep {
    /// Returns all steps in the main "create new identity" flow, in order.
    ///
    /// Side-flows (`DeviceLinkInstructions`, `BackupPasswordEntry`) are
    /// intentionally excluded — they branch from `IdentityCheck` and do
    /// not advance the main progress indicator.
    pub fn all() -> &'static [OnboardingStep] {
        &[
            OnboardingStep::IdentityCheck,
            OnboardingStep::DefaultName,
            OnboardingStep::GroupsSetup,
            OnboardingStep::ContactInfo,
            OnboardingStep::WhatNext,
        ]
    }

    /// Returns the zero-based index of this step in the main flow.
    ///
    /// Side-flows off `IdentityCheck` (`DeviceLinkInstructions`,
    /// `BackupPasswordEntry`) reuse `IdentityCheck`'s index so the progress
    /// indicator stays accurate for the create-identity path.
    pub fn index(&self) -> usize {
        match self {
            OnboardingStep::IdentityCheck => 0,
            // Side-flows off IdentityCheck — reuse its index so the 5-step
            // progress indicator stays accurate.
            OnboardingStep::DeviceLinkInstructions => 0,
            OnboardingStep::BackupPasswordEntry => 0,
            OnboardingStep::DefaultName => 1,
            OnboardingStep::GroupsSetup => 2,
            OnboardingStep::ContactInfo => 3,
            OnboardingStep::WhatNext => 4,
        }
    }

    /// Returns the next step, or `None` if this is the final step.
    pub fn next(&self) -> Option<OnboardingStep> {
        let all = Self::all();
        let idx = self.index();
        all.get(idx + 1).copied()
    }

    /// Returns the previous step, or `None` if this is the first step.
    pub fn previous(&self) -> Option<OnboardingStep> {
        let idx = self.index();
        if idx == 0 {
            None
        } else {
            Some(Self::all()[idx - 1])
        }
    }

    /// Returns the total number of steps.
    pub fn total() -> usize {
        Self::all().len()
    }
}

impl Default for OnboardingProgress {
    /// `Default` stamps `started_at` as 0 (Unix epoch). Production
    /// constructors call `OnboardingProgress::new(now)` directly; the
    /// `Default` impl is retained for serde and test ergonomics.
    fn default() -> Self {
        Self::new(0)
    }
}

impl OnboardingProgress {
    /// Creates a new onboarding progress starting at `IdentityCheck`.
    /// Creates a new onboarding progress starting at `IdentityCheck`.
    ///
    /// `now` is the Unix-epoch timestamp to stamp into
    /// `started_at`. Vauchi reads this from `self.clock.unix_seconds()`;
    /// tests pass any fixed value.
    pub fn new(now: u64) -> Self {
        Self {
            current_step: OnboardingStep::IdentityCheck,
            completed_steps: HashSet::new(),
            started_at: Some(now),
            completed_at: None,
            skipped_backup: false,
        }
    }

    /// Advances to the next step in the wizard.
    ///
    /// Marks the current step as completed and moves to the next one.
    /// If already at the final step (`WhatNext`), this is idempotent and
    /// marks the onboarding as complete.
    ///
    /// Returns the new current step.
    pub fn advance(&mut self, now: u64) -> OnboardingStep {
        self.completed_steps.insert(self.current_step);

        if let Some(next) = self.current_step.next() {
            self.current_step = next;
        } else if self.completed_at.is_none() {
            // Already at the final step — stamp completion.
            self.completed_at = Some(now);
        }

        self.current_step
    }

    /// Skips the current step without marking it as completed.
    ///
    /// Moves to the next step. If already at the final step, this is idempotent.
    ///
    /// Returns the new current step.
    pub fn skip_step(&mut self, now: u64) -> OnboardingStep {
        if let Some(next) = self.current_step.next() {
            self.current_step = next;
        } else if self.completed_at.is_none() {
            // Already at the final step — stamp completion.
            self.completed_at = Some(now);
        }

        self.current_step
    }

    /// Returns whether onboarding is complete (reached and passed `Ready`).
    pub fn is_complete(&self) -> bool {
        self.completed_at.is_some()
    }

    /// Stamps the progress as complete without changing the current
    /// step. Idempotent. Used by `Vauchi::mark_onboarding_complete`
    /// to atomically pair identity creation with onboarding
    /// completion (closes the crash window between
    /// `create_identity` and `set_onboarding_completed` that the
    /// audit `2026-04-28-app-launch-and-identity-orchestration-in-core`
    /// §2.5 calls out).
    pub fn mark_complete(&mut self, now: u64) {
        if self.completed_at.is_none() {
            self.completed_at = Some(now);
        }
    }

    /// Resets onboarding to the beginning.
    ///
    /// Clears all progress, timestamps, and flags.
    pub fn reset(&mut self, now: u64) {
        self.current_step = OnboardingStep::IdentityCheck;
        self.completed_steps.clear();
        self.started_at = Some(now);
        self.completed_at = None;
        self.skipped_backup = false;
    }

    /// Returns the current step.
    pub fn current_step(&self) -> OnboardingStep {
        self.current_step
    }

    /// Returns the completion percentage (0 to 100).
    pub fn completion_percentage(&self) -> u8 {
        let total = OnboardingStep::total();
        if total == 0 {
            return 0;
        }

        let completed = self.completed_steps.len();
        let pct = (completed * 100) / total;
        pct.min(100) as u8
    }

    /// Serialize to JSON for storage.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Generate display name suggestions from a full name.
///
/// Given "Alexandra Johnson", returns:
/// - "Alexandra" (first name)
/// - "Alex" (shortened first name, if 5+ chars)
/// - "A. Johnson" (initial + last name)
pub fn display_name_suggestions(full_name: &str) -> Vec<String> {
    let normalized = normalize_text(full_name);
    let parts: Vec<&str> = normalized.split_whitespace().collect();

    if parts.is_empty() {
        return vec![];
    }

    let mut suggestions = Vec::new();

    let first = parts[0];
    if !first.is_empty() {
        suggestions.push(first.to_string());
    }

    if first.chars().count() >= 5 {
        let short: String = first.chars().take(4).collect();
        if short != first {
            suggestions.push(short);
        }
    }

    if parts.len() >= 2 {
        let Some(initial) = first.chars().next() else {
            return suggestions;
        };
        let last = parts[parts.len() - 1];
        suggestions.push(format!("{}. {}", initial, last));
    }

    suggestions
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

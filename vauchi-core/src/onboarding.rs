// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding State Machine
//!
//! Tracks user progress through the first-run onboarding wizard.
//! Persisted as encrypted UX state; exposed via `Vauchi` API and UniFFI.
//!
//! Feature file: features/onboarding.feature

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Steps in the onboarding wizard.
///
/// The user progresses through these in order, though backward
/// transitions are always allowed and some steps can be skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum OnboardingStep {
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

impl OnboardingStep {
    /// Returns all steps in order.
    pub fn all() -> &'static [OnboardingStep] {
        &[
            OnboardingStep::Welcome,
            OnboardingStep::DefaultName,
            OnboardingStep::SkipGate,
            OnboardingStep::GroupsSetup,
            OnboardingStep::ContactInfo,
            OnboardingStep::PreviewCard,
            OnboardingStep::SecurityExplanation,
            OnboardingStep::BackupPrompt,
            OnboardingStep::Ready,
        ]
    }

    /// Returns the zero-based index of this step in the wizard.
    pub fn index(&self) -> usize {
        match self {
            OnboardingStep::Welcome => 0,
            OnboardingStep::DefaultName => 1,
            OnboardingStep::SkipGate => 2,
            OnboardingStep::GroupsSetup => 3,
            OnboardingStep::ContactInfo => 4,
            OnboardingStep::PreviewCard => 5,
            OnboardingStep::SecurityExplanation => 6,
            OnboardingStep::BackupPrompt => 7,
            OnboardingStep::Ready => 8,
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

/// Tracks the user's progress through the onboarding wizard.
///
/// Follows the same persistence pattern as `DemoContactState` and
/// `AhaMomentTracker` — serialized to JSON, encrypted, and stored
/// in the `ux_state` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingProgress {
    /// The step the user is currently on.
    pub current_step: OnboardingStep,
    /// Steps that have been completed (visited and passed).
    pub completed_steps: HashSet<OnboardingStep>,
    /// Timestamp when onboarding was started (Unix epoch seconds).
    pub started_at: Option<u64>,
    /// Timestamp when onboarding was completed (Unix epoch seconds).
    pub completed_at: Option<u64>,
    /// Whether the user skipped the backup step.
    pub skipped_backup: bool,
}

impl Default for OnboardingProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingProgress {
    /// Creates a new onboarding progress starting at `Welcome`.
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        Self {
            current_step: OnboardingStep::Welcome,
            completed_steps: HashSet::new(),
            started_at: Some(now),
            completed_at: None,
            skipped_backup: false,
        }
    }

    /// Advances to the next step in the wizard.
    ///
    /// Marks the current step as completed and moves to the next one.
    /// If already at the final step (`Ready`), this is idempotent and
    /// marks the onboarding as complete.
    ///
    /// Returns the new current step.
    pub fn advance(&mut self) -> OnboardingStep {
        self.completed_steps.insert(self.current_step);

        if let Some(next) = self.current_step.next() {
            self.current_step = next;
        } else {
            // Already at Ready — mark as complete
            if self.completed_at.is_none() {
                self.completed_at = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_secs(),
                );
            }
        }

        self.current_step
    }

    /// Skips the current step without marking it as completed.
    ///
    /// If the current step is `BackupPrompt`, records that backup was skipped.
    /// Moves to the next step. If already at the final step, this is idempotent.
    ///
    /// Returns the new current step.
    pub fn skip_step(&mut self) -> OnboardingStep {
        if self.current_step == OnboardingStep::BackupPrompt {
            self.skipped_backup = true;
        }

        if let Some(next) = self.current_step.next() {
            self.current_step = next;
        } else {
            // Already at Ready — mark as complete
            if self.completed_at.is_none() {
                self.completed_at = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_secs(),
                );
            }
        }

        self.current_step
    }

    /// Skips from SkipGate directly to SecurityExplanation.
    /// Called when user chooses "Skip to finish" at the skip gate.
    pub fn skip_to_finish(&mut self) {
        self.current_step = OnboardingStep::SecurityExplanation;
    }

    /// Returns whether onboarding is complete (reached and passed `Ready`).
    pub fn is_complete(&self) -> bool {
        self.completed_at.is_some()
    }

    /// Resets onboarding to the beginning.
    ///
    /// Clears all progress, timestamps, and flags.
    pub fn reset(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        self.current_step = OnboardingStep::Welcome;
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
    let parts: Vec<&str> = full_name.split_whitespace().collect();

    if parts.is_empty() {
        return vec![];
    }

    let mut suggestions = Vec::new();

    // First name
    let first = parts[0];
    if !first.is_empty() {
        suggestions.push(first.to_string());
    }

    // Shortened first name (if 5+ chars, take first 4 chars boundary)
    if first.chars().count() >= 5 {
        // Find a character boundary at approximately 4 chars
        let short: String = first.chars().take(4).collect();
        // Only add if it's different from the first name
        if short != first {
            suggestions.push(short);
        }
    }

    // Initial + last name (only if there are multiple parts)
    if parts.len() >= 2 {
        let initial = first.chars().next().unwrap();
        let last = parts[parts.len() - 1];
        suggestions.push(format!("{}. {}", initial, last));
    }

    suggestions
}

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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::text::normalize_text;
use crate::types::{OnboardingProgress, OnboardingStep};

impl OnboardingStep {
    /// Returns all steps in order.
    pub fn all() -> &'static [OnboardingStep] {
        &[
            OnboardingStep::IdentityCheck,
            OnboardingStep::LinkChoice,
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
            OnboardingStep::IdentityCheck => 0,
            OnboardingStep::LinkChoice => 1,
            OnboardingStep::Welcome => 2,
            OnboardingStep::DefaultName => 3,
            OnboardingStep::SkipGate => 4,
            OnboardingStep::GroupsSetup => 5,
            OnboardingStep::ContactInfo => 6,
            OnboardingStep::PreviewCard => 7,
            OnboardingStep::SecurityExplanation => 8,
            OnboardingStep::BackupPrompt => 9,
            OnboardingStep::Ready => 10,
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
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingProgress {
    /// Creates a new onboarding progress starting at `IdentityCheck`.
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

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
    /// No-op if not currently at SkipGate (prevents state corruption).
    pub fn skip_to_finish(&mut self) {
        if self.current_step != OnboardingStep::SkipGate {
            return;
        }
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
        let Some(initial) = first.chars().next() else {
            return suggestions;
        };
        let last = parts[parts.len() - 1];
        suggestions.push(format!("{}. {}", initial, last));
    }

    suggestions
}

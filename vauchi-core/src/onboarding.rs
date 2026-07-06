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
use crate::types::{OnboardingProgress, OnboardingStep};

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

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding API operations.
//!
//! Provides methods on `Vauchi<T>` for querying and mutating onboarding
//! progress. The state machine lives in `crate::onboarding`; this module
//! wires it to storage and events.

use crate::contact::Group;

use crate::types::{OnboardingProgress, OnboardingStep};

use super::super::error::VauchiResult;
use super::Vauchi;

impl Vauchi {
    // === Onboarding Progress Operations ===

    /// Returns the current onboarding progress.
    ///
    /// Loads from storage if available, otherwise returns a fresh instance.
    pub fn get_onboarding_progress(&self) -> VauchiResult<OnboardingProgress> {
        Ok(self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?)
    }

    /// Advances onboarding to the next step.
    ///
    /// Marks the current step as completed and moves forward.
    /// Persists the updated progress to storage.
    /// Returns the updated progress.
    pub fn advance_onboarding(&self) -> VauchiResult<OnboardingProgress> {
        let mut progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        progress.advance(self.clock.unix_seconds());
        self.storage.save_onboarding_progress(&progress)?;
        Ok(progress)
    }

    /// Skips the current onboarding step without marking it completed.
    ///
    /// Persists the updated progress to storage.
    /// Returns the updated progress.
    pub fn skip_onboarding_step(&self) -> VauchiResult<OnboardingProgress> {
        let mut progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        progress.skip_step(self.clock.unix_seconds());
        self.storage.save_onboarding_progress(&progress)?;
        Ok(progress)
    }

    /// Resets onboarding to the beginning.
    ///
    /// Clears all progress and starts fresh. Useful for "replay onboarding"
    /// from settings.
    pub fn reset_onboarding(&self) -> VauchiResult<()> {
        let mut progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        progress.reset(self.clock.unix_seconds());
        self.storage.save_onboarding_progress(&progress)?;
        Ok(())
    }

    /// Returns whether onboarding has been completed.
    pub fn is_onboarding_complete(&self) -> VauchiResult<bool> {
        let progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        Ok(progress.is_complete())
    }

    /// Marks onboarding as complete, irrespective of which step the
    /// progress is currently on.
    ///
    /// Idempotent — calling on already-complete progress is a no-op.
    /// Used by [`Vauchi::create_identity_with_onboarding`] so an
    /// atomic create-and-complete can be expressed as one FFI call,
    /// closing the crash window the audit
    /// `2026-04-28-app-launch-and-identity-orchestration-in-core`
    /// §2.5 calls out.
    pub fn mark_onboarding_complete(&self) -> VauchiResult<()> {
        let mut progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        if progress.is_complete() {
            return Ok(());
        }
        progress.mark_complete(self.clock.unix_seconds());
        self.storage.save_onboarding_progress(&progress)?;
        Ok(())
    }

    /// Returns the current onboarding step.
    pub fn current_onboarding_step(&self) -> VauchiResult<OnboardingStep> {
        let progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        Ok(progress.current_step())
    }

    /// Returns the onboarding completion percentage (0-100).
    pub fn onboarding_completion_percentage(&self) -> VauchiResult<u8> {
        let progress = self
            .storage
            .load_or_create_onboarding_progress(self.clock.unix_seconds())?;
        Ok(progress.completion_percentage())
    }

    /// Creates groups from the given list of names during onboarding.
    ///
    /// Used during onboarding step 4 (Groups Setup). Skips names that
    /// already exist as labels. Persists each label to storage.
    /// Returns the list of newly created labels.
    pub fn create_suggested_groups(&self, names: &[&str]) -> VauchiResult<Vec<Group>> {
        let mut created = Vec::new();
        for name in names {
            match self.storage.create_group(name) {
                Ok(label) => created.push(label),
                Err(crate::StorageError::AlreadyExists(_)) => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(created)
    }
}

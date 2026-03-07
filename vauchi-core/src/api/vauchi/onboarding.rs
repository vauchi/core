// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding API operations.
//!
//! Provides methods on `Vauchi<T>` for querying and mutating onboarding
//! progress. The state machine lives in `crate::onboarding`; this module
//! wires it to storage and events.

use crate::network::Transport;
use crate::onboarding::{OnboardingProgress, OnboardingStep};

use super::super::error::VauchiResult;
use super::Vauchi;

impl<T: Transport> Vauchi<T> {
    // === Onboarding Progress Operations ===

    /// Returns the current onboarding progress.
    ///
    /// Loads from storage if available, otherwise returns a fresh instance.
    pub fn get_onboarding_progress(&self) -> VauchiResult<OnboardingProgress> {
        Ok(self.storage.load_or_create_onboarding_progress()?)
    }

    /// Advances onboarding to the next step.
    ///
    /// Marks the current step as completed and moves forward.
    /// Persists the updated progress to storage.
    /// Returns the updated progress.
    pub fn advance_onboarding(&self) -> VauchiResult<OnboardingProgress> {
        let mut progress = self.storage.load_or_create_onboarding_progress()?;
        progress.advance();
        self.storage.save_onboarding_progress(&progress)?;
        Ok(progress)
    }

    /// Skips the current onboarding step without marking it completed.
    ///
    /// If the current step is `BackupPrompt`, records that backup was skipped.
    /// Persists the updated progress to storage.
    /// Returns the updated progress.
    pub fn skip_onboarding_step(&self) -> VauchiResult<OnboardingProgress> {
        let mut progress = self.storage.load_or_create_onboarding_progress()?;
        progress.skip_step();
        self.storage.save_onboarding_progress(&progress)?;
        Ok(progress)
    }

    /// Skips from SkipGate to SecurityExplanation (skip gate "Skip to finish").
    pub fn skip_onboarding_to_finish(&self) -> VauchiResult<OnboardingProgress> {
        let mut progress = self.storage.load_or_create_onboarding_progress()?;
        progress.skip_to_finish();
        self.storage.save_onboarding_progress(&progress)?;
        Ok(progress)
    }

    /// Resets onboarding to the beginning.
    ///
    /// Clears all progress and starts fresh. Useful for "replay onboarding"
    /// from settings.
    pub fn reset_onboarding(&self) -> VauchiResult<()> {
        let mut progress = self.storage.load_or_create_onboarding_progress()?;
        progress.reset();
        self.storage.save_onboarding_progress(&progress)?;
        Ok(())
    }

    /// Returns whether onboarding has been completed.
    pub fn is_onboarding_complete(&self) -> VauchiResult<bool> {
        let progress = self.storage.load_or_create_onboarding_progress()?;
        Ok(progress.is_complete())
    }

    /// Returns the current onboarding step.
    pub fn current_onboarding_step(&self) -> VauchiResult<OnboardingStep> {
        let progress = self.storage.load_or_create_onboarding_progress()?;
        Ok(progress.current_step())
    }

    /// Returns the onboarding completion percentage (0-100).
    pub fn onboarding_completion_percentage(&self) -> VauchiResult<u8> {
        let progress = self.storage.load_or_create_onboarding_progress()?;
        Ok(progress.completion_percentage())
    }
}

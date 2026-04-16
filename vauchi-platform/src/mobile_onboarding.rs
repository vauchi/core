// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding operations for mobile.

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{MobileOnboardingProgress, MobileOnboardingStep};

#[uniffi::export]
impl VauchiPlatform {
    // === Onboarding Operations ===

    /// Get the current onboarding progress.
    pub fn get_onboarding_progress(&self) -> Result<MobileOnboardingProgress, MobileError> {
        let storage = self.open_storage()?;
        let progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(MobileOnboardingProgress::from(&progress))
    }

    /// Advance onboarding to the next step.
    ///
    /// Marks the current step as completed and moves forward.
    /// Returns the updated progress.
    pub fn advance_onboarding(&self) -> Result<MobileOnboardingProgress, MobileError> {
        let storage = self.open_storage()?;
        let mut progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        progress.advance();
        storage
            .save_onboarding_progress(&progress)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(MobileOnboardingProgress::from(&progress))
    }

    /// Skip the current onboarding step without marking it completed.
    ///
    /// Returns the updated progress.
    pub fn skip_onboarding_step(&self) -> Result<MobileOnboardingProgress, MobileError> {
        let storage = self.open_storage()?;
        let mut progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        progress.skip_step();
        storage
            .save_onboarding_progress(&progress)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(MobileOnboardingProgress::from(&progress))
    }

    /// Reset onboarding to the beginning.
    ///
    /// Useful for "replay onboarding" from settings.
    pub fn reset_onboarding(&self) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let mut progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        progress.reset();
        storage
            .save_onboarding_progress(&progress)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(())
    }

    /// Check if onboarding has been completed.
    pub fn is_onboarding_complete(&self) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(progress.is_complete())
    }

    /// Get the current onboarding step.
    pub fn current_onboarding_step(&self) -> Result<MobileOnboardingStep, MobileError> {
        let storage = self.open_storage()?;
        let progress = storage
            .load_or_create_onboarding_progress()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(progress.current_step().into())
    }

    /// Get display name suggestions from a full name.
    ///
    /// Given "Alexandra Johnson", returns suggestions like
    /// "Alexandra", "Alex", "A. Johnson".
    pub fn display_name_suggestions(&self, full_name: String) -> Vec<String> {
        vauchi_core::display_name_suggestions(&full_name)
    }
}

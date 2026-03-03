// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding and setup progress operations.
//!
//! Provides a single API to query how far through initial setup the user is,
//! combining identity creation, card completion, first contact, and device
//! linking into a unified `SetupProgress` struct.

use crate::network::Transport;

use super::super::error::VauchiResult;
use super::Vauchi;

/// Represents the user's onboarding progress.
///
/// Each field tracks whether a specific setup step has been completed.
/// Clients use this to render onboarding checklists and progress indicators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupProgress {
    /// Whether the user has created an identity.
    pub identity_created: bool,
    /// Whether the user's card has at least one field (besides display name).
    pub card_has_fields: bool,
    /// Whether the user has added at least one real contact.
    pub has_contacts: bool,
    /// Whether the user has at least three contacts.
    pub has_three_contacts: bool,
    /// Whether a device has been linked (device registry has >1 device).
    pub device_linked: bool,
    /// Whether an app password has been configured.
    pub password_set: bool,
    /// Number of completed steps out of total.
    pub completed_steps: usize,
    /// Total number of setup steps tracked.
    pub total_steps: usize,
}

impl SetupProgress {
    /// Returns a fraction (0.0 to 1.0) representing completion.
    pub fn completion_fraction(&self) -> f64 {
        if self.total_steps == 0 {
            return 0.0;
        }
        self.completed_steps as f64 / self.total_steps as f64
    }

    /// Returns true if all setup steps are complete.
    pub fn is_complete(&self) -> bool {
        self.completed_steps == self.total_steps
    }
}

impl<T: Transport> Vauchi<T> {
    /// Returns the current onboarding/setup progress.
    ///
    /// Aggregates multiple state checks into a single struct that clients
    /// can use for onboarding checklists and progress indicators.
    pub fn get_setup_progress(&self) -> VauchiResult<SetupProgress> {
        let identity_created = self.has_identity();

        let card_has_fields = match self.storage.load_own_card()? {
            Some(card) => !card.fields().is_empty(),
            None => false,
        };

        let contact_count = self.storage.list_contacts()?.len();
        let has_contacts = contact_count > 0;
        let has_three_contacts = contact_count >= 3;

        let device_linked = match self.storage.load_device_registry()? {
            Some(registry) => registry.device_count() > 1,
            None => false,
        };

        let password_set = self.storage.load_password_config()?.is_some();

        // Count completed steps
        let steps = [
            identity_created,
            card_has_fields,
            has_contacts,
            has_three_contacts,
            device_linked,
            password_set,
        ];
        let completed_steps = steps.iter().filter(|&&s| s).count();
        let total_steps = steps.len();

        Ok(SetupProgress {
            identity_created,
            card_has_fields,
            has_contacts,
            has_three_contacts,
            device_linked,
            password_set,
            completed_steps,
            total_steps,
        })
    }

    /// Returns true if this appears to be the first launch.
    ///
    /// First launch is defined as: no identity created and no contacts.
    pub fn is_first_launch(&self) -> VauchiResult<bool> {
        let has_id = self.has_identity();
        if has_id {
            return Ok(false);
        }

        // Also check storage — identity might be persisted but not loaded
        let has_stored_id = self.storage.has_identity()?;
        if has_stored_id {
            return Ok(false);
        }

        Ok(true)
    }
}

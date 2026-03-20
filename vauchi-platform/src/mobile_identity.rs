// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity, aha moments, and demo contact operations for mobile.

use vauchi_core::{ContactCard, Identity};

use super::error::MobileError;
use super::types::{
    MobileAhaMoment, MobileAhaMomentType, MobileDemoContact, MobileDemoContactState,
};
use super::{IdentityData, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === Identity Operations ===

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return true;
            }
        }

        if let Ok(storage) = self.open_storage()
            && let Ok(Some((backup_data, display_name))) = storage.load_identity()
        {
            let identity_data = IdentityData {
                backup_data,
                display_name,
            };
            *self.identity_data.lock().unwrap() = Some(identity_data);
            return true;
        }

        false
    }

    /// Create a new identity.
    pub fn create_identity(&self, display_name: String) -> Result<(), MobileError> {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        let identity = Identity::create(&display_name);

        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let backup_data = backup.as_bytes().to_vec();

        let storage = self.open_storage()?;
        storage.save_identity(&backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        let card = ContactCard::new(&display_name);
        storage.save_own_card(&card)?;

        Ok(())
    }

    /// Get public ID.
    pub fn get_public_id(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.public_id())
    }

    /// Get formatted fingerprint of own identity public key.
    ///
    /// Returns the fingerprint as 16 groups of 4 uppercase hex characters,
    /// suitable for display and manual comparison with contacts.
    pub fn get_own_fingerprint(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        let hex = hex::encode(identity.signing_public_key());
        Ok(hex
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase())
    }

    /// Get display name.
    pub fn get_display_name(&self) -> Result<String, MobileError> {
        let storage = self.open_storage()?;
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        Ok(card.display_name().to_string())
    }

    // === Aha Moments (public API) ===

    /// Check if an aha moment has been seen.
    pub fn has_seen_aha_moment(&self, moment_type: MobileAhaMomentType) -> bool {
        let tracker = self.load_aha_tracker();
        tracker.has_seen(moment_type.into())
    }

    /// Try to trigger an aha moment. Returns the moment if not yet seen, None otherwise.
    pub fn try_trigger_aha_moment(
        &self,
        moment_type: MobileAhaMomentType,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger(core_type) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Try to trigger an aha moment with context (e.g., contact name).
    pub fn try_trigger_aha_moment_with_context(
        &self,
        moment_type: MobileAhaMomentType,
        context: String,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger_with_context(core_type, context) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the count of seen aha moments.
    pub fn aha_moments_seen_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.seen_count() as u32
    }

    /// Get the total count of aha moments.
    pub fn aha_moments_total_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.total_count() as u32
    }

    /// Reset all aha moments (for testing/debugging).
    pub fn reset_aha_moments(&self) -> Result<(), MobileError> {
        let mut tracker = self.load_aha_tracker();
        tracker.reset();
        self.save_aha_tracker(&tracker)
    }

    // === Demo Contact (public API) ===

    /// Initialize the demo contact if user has no real contacts.
    /// Call this after onboarding completes.
    pub fn init_demo_contact_if_needed(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        if !contacts.is_empty() {
            return Ok(None);
        }

        let mut state = self.load_demo_state();
        if state.was_dismissed || state.auto_removed {
            return Ok(None);
        }

        if !state.is_active {
            state = vauchi_core::DemoContactState::new_active();
            self.save_demo_state(&state)?;
        }

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the current demo contact if active.
    pub fn get_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the demo contact state.
    pub fn get_demo_contact_state(&self) -> MobileDemoContactState {
        let state = self.load_demo_state();
        MobileDemoContactState {
            is_active: state.is_active,
            was_dismissed: state.was_dismissed,
            auto_removed: state.auto_removed,
            update_count: state.update_count,
        }
    }

    /// Check if a demo update is available.
    pub fn is_demo_update_available(&self) -> bool {
        let state = self.load_demo_state();
        state.is_update_due()
    }

    /// Trigger a demo update and get the new content.
    pub fn trigger_demo_update(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.advance_to_next_tip() {
            self.save_demo_state(&state)?;
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Dismiss the demo contact.
    pub fn dismiss_demo_contact(&self) -> Result<(), MobileError> {
        let mut state = self.load_demo_state();
        state.dismiss();
        self.save_demo_state(&state)
    }

    /// Auto-remove demo contact after first real exchange.
    /// Call this after a successful contact exchange.
    pub fn auto_remove_demo_contact(&self) -> Result<bool, MobileError> {
        let mut state = self.load_demo_state();
        if state.is_active {
            state.auto_remove();
            self.save_demo_state(&state)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Restore the demo contact from Settings.
    pub fn restore_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        state.restore();
        self.save_demo_state(&state)?;

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }
}

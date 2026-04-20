// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Password, duress, emergency broadcast, and decoy contact operations for mobile.

use vauchi_core::ContactCard;

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{
    MobileAuthMode, MobileBroadcastResult, MobileDecoyContact, MobileDuressSettings,
    MobileEmergencyConfig,
};

#[uniffi::export]
impl VauchiPlatform {
    // === App Password / Duress PIN ===

    /// Sets up an app password (PIN).
    ///
    /// Requires an identity to be created first.
    pub fn setup_app_password(&self, password: String) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        vauchi.setup_app_password(&password)?;
        Ok(())
    }

    /// Sets up a duress PIN.
    ///
    /// Requires an app password to be configured first.
    pub fn setup_duress_password(&self, duress_password: String) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        vauchi.setup_duress_password(&duress_password)?;
        Ok(())
    }

    /// Authenticates with a password.
    ///
    /// Returns the authentication mode:
    /// - `Normal` if the real password matches
    /// - `Duress` if the duress PIN matches
    /// - Returns an error if neither matches
    pub fn authenticate(&self, password: String) -> Result<MobileAuthMode, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let mode = vauchi.authenticate(&password)?;
        match mode {
            vauchi_core::AuthMode::Normal => Ok(MobileAuthMode::Normal),
            vauchi_core::AuthMode::Duress => Ok(MobileAuthMode::Duress),
            vauchi_core::AuthMode::Unauthenticated => Ok(MobileAuthMode::Normal),
            _ => Ok(MobileAuthMode::Normal),
        }
    }

    /// Returns whether an app password has been configured.
    pub fn is_password_enabled(&self) -> Result<bool, MobileError> {
        let vauchi = self.open_vauchi()?;
        Ok(vauchi.is_password_enabled()?)
    }

    /// Returns whether duress mode is enabled.
    pub fn is_duress_enabled(&self) -> Result<bool, MobileError> {
        let vauchi = self.open_vauchi()?;
        Ok(vauchi.is_duress_enabled()?)
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.disable_duress()?;
        Ok(())
    }

    // === Duress Settings ===

    /// Configures duress alert settings.
    ///
    /// Sets which contacts receive alerts, the alert message, and
    /// whether to include device location.
    pub fn configure_duress_alerts(
        &self,
        contact_ids: Vec<String>,
        message: String,
    ) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        let settings = vauchi_core::DuressSettings {
            alert_contact_ids: contact_ids,
            alert_message: message,
            include_location: false,
        };
        vauchi.save_duress_settings(&settings)?;
        Ok(())
    }

    /// Gets the current duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn get_duress_settings(&self) -> Result<Option<MobileDuressSettings>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let settings = vauchi.load_duress_settings()?;
        Ok(settings.map(|s| MobileDuressSettings {
            alert_contact_ids: s.alert_contact_ids,
            alert_message: s.alert_message,
            include_location: s.include_location,
        }))
    }

    // === Emergency Broadcast ===

    /// Configures the emergency broadcast system.
    ///
    /// Sets which contacts receive emergency alerts, the alert message,
    /// and whether to include device location.
    pub fn configure_emergency_broadcast(
        &self,
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    ) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        vauchi.configure_emergency_broadcast(contact_ids, message, include_location)?;
        Ok(())
    }

    /// Sends an emergency broadcast to all trusted contacts.
    ///
    /// Returns the number of alerts sent and total configured.
    pub fn send_emergency_broadcast(&self) -> Result<MobileBroadcastResult, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let result = vauchi.send_emergency_broadcast()?;
        Ok(MobileBroadcastResult {
            sent: result.sent as u32,
            total: result.total as u32,
        })
    }

    /// Gets the current emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn get_emergency_config(&self) -> Result<Option<MobileEmergencyConfig>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let config = vauchi.load_emergency_config()?;
        Ok(config.map(|c| MobileEmergencyConfig {
            trusted_contact_ids: c.trusted_contact_ids,
            message: c.message,
            include_location: c.include_location,
        }))
    }

    /// Disables the emergency broadcast by deleting the configuration.
    pub fn disable_emergency_broadcast(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.delete_emergency_config()?;
        Ok(())
    }

    // === Decoy Contacts ===

    /// Adds a decoy contact for duress mode.
    ///
    /// The card_json should be a JSON-serialized ContactCard.
    /// Returns the generated ID.
    pub fn add_decoy_contact(
        &self,
        name: String,
        card_json: String,
    ) -> Result<String, MobileError> {
        let vauchi = self.open_vauchi()?;
        let card: ContactCard =
            serde_json::from_str(&card_json).map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let id = format!(
            "decoy-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        vauchi.add_decoy_contact(&id, &name, &card)?;
        Ok(id)
    }

    /// Lists all decoy contacts.
    pub fn list_decoy_contacts(&self) -> Result<Vec<MobileDecoyContact>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let decoys = vauchi.list_decoy_contacts()?;
        Ok(decoys
            .into_iter()
            .map(|(id, display_name, _card)| MobileDecoyContact { id, display_name })
            .collect())
    }

    /// Deletes a decoy contact by ID.
    pub fn delete_decoy_contact(&self, id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.remove_decoy_contact(&id)?;
        Ok(())
    }
}

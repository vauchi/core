// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery status, events, app password, duress PIN, and duress settings.

use super::super::app_password::{AppPasswordConfig, AuthResult};
use super::super::duress::{DuressAlert, DuressAlertType};
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::{EventCallback, VauchiEvent};
use super::{AuthMode, Vauchi};
use crate::storage::ActivityLogRow;
use crate::types::DuressSettings;

impl Vauchi {
    // === Delivery Status Operations ===

    /// Gets delivery records for a specific contact.
    ///
    /// Returns all delivery records where the given contact is the recipient,
    /// ordered by creation time (most recent first).
    pub fn get_delivery_status_for_contact(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::storage::DeliveryRecord>> {
        Ok(self
            .storage
            .get_delivery_records_for_recipient(contact_id)?)
    }

    /// Gets all failed delivery records across all contacts.
    ///
    /// Returns delivery records with `Failed` status, useful for showing
    /// the user which messages need attention or retry.
    pub fn get_failed_deliveries(&self) -> VauchiResult<Vec<crate::storage::DeliveryRecord>> {
        Ok(self.storage.get_delivery_records_by_status(
            &crate::storage::DeliveryStatus::Failed {
                reason: String::new(),
            },
        )?)
    }

    // === Event Operations ===

    /// Adds an event handler (#87, #94).
    ///
    /// Returns the handler ID which can be used with `remove_event_handler()`.
    /// No longer requires `&mut self` — registration works even when the
    /// dispatcher is shared with SyncController.
    pub fn add_event_handler(&self, handler: EventCallback) -> crate::api::events::HandlerId {
        self.events.add_handler(handler)
    }

    /// Removes an event handler by its ID (#89).
    /// Returns true if the handler was found and removed.
    pub fn remove_event_handler(&self, id: crate::api::events::HandlerId) -> bool {
        self.events.remove_handler(id)
    }

    /// Clears all event handlers.
    pub fn clear_event_handlers(&self) {
        self.events.clear_handlers();
    }

    /// Dispatches an event to all handlers.
    pub fn dispatch_event(&self, event: VauchiEvent) {
        self.events.dispatch(event);
    }

    // === App Password / Duress PIN ===

    /// Returns the current authentication mode.
    pub fn auth_mode(&self) -> AuthMode {
        self.auth_mode
    }

    /// Authenticates with a password.
    ///
    /// Loads the password configuration from storage, verifies the password,
    /// and sets the auth mode accordingly:
    /// - `Normal` if the real password matches
    /// - `Duress` if the duress PIN matches
    /// - Returns an error if neither matches
    pub fn authenticate(&mut self, password: &str) -> VauchiResult<AuthMode> {
        let config = self
            .storage
            .load_password_config()?
            .ok_or_else(|| VauchiError::InvalidState("no password configured".into()))?;

        match config.verify(password) {
            AuthResult::Normal => {
                self.auth_mode = AuthMode::Normal;
                Ok(AuthMode::Normal)
            }
            AuthResult::Duress => {
                self.auth_mode = AuthMode::Duress;
                self.queue_duress_alert()?;
                Ok(AuthMode::Duress)
            }
            AuthResult::Invalid => Err(VauchiError::InvalidState("invalid password".into())),
        }
    }

    /// Sets up an app password (PIN).
    ///
    /// Requires an identity to be created first (the password columns
    /// live on the `identity` table). If the identity row doesn't exist
    /// in the database yet, it is created with a placeholder.
    pub fn setup_app_password(&mut self, password: &str) -> VauchiResult<()> {
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }

        // Ensure the identity row exists in DB (may not yet if create_identity
        // only stored the own_card). Insert a placeholder row if missing.
        if !self.storage.has_identity()? {
            self.storage.save_identity(b"", "")?;
        }

        let config = AppPasswordConfig::create(password)?;
        self.storage
            .save_app_password(config.password_hash(), config.password_salt())?;

        Ok(())
    }

    /// Sets up a duress PIN.
    ///
    /// Requires an app password to be configured first.
    pub fn setup_duress_password(&mut self, duress_password: &str) -> VauchiResult<()> {
        let mut config = self.storage.load_password_config()?.ok_or_else(|| {
            VauchiError::InvalidState("app password must be set before duress PIN".into())
        })?;

        config.setup_duress(duress_password)?;

        let duress_hash = config
            .duress_hash()
            .ok_or_else(|| VauchiError::InvalidState("duress hash not set".into()))?;
        let duress_salt = config
            .duress_salt()
            .ok_or_else(|| VauchiError::InvalidState("duress salt not set".into()))?;

        self.storage
            .save_duress_password(duress_hash, duress_salt)?;

        Ok(())
    }

    /// Returns whether an app password has been configured.
    pub fn is_password_enabled(&self) -> VauchiResult<bool> {
        Ok(self.storage.load_password_config()?.is_some())
    }

    /// Returns the activity log entries newer than the given timestamp.
    /// Used for OS notification polling.
    pub fn activity_log_poll(&self, since_secs: u64) -> VauchiResult<Vec<ActivityLogRow>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let max_age = now.saturating_sub(since_secs);
        Ok(self.storage.activity_log_query_recent(now, max_age)?)
    }

    /// Returns whether duress mode is enabled.
    pub fn is_duress_enabled(&self) -> VauchiResult<bool> {
        match self.storage.load_password_config()? {
            Some(config) => Ok(config.duress_enabled()),
            None => Ok(false),
        }
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&mut self) -> VauchiResult<()> {
        self.storage.disable_duress()?;
        Ok(())
    }

    // === Duress Settings ===

    /// Saves duress alert settings (trusted contacts, message, location).
    pub fn save_duress_settings(&self, settings: &DuressSettings) -> VauchiResult<()> {
        self.storage.save_duress_settings(settings)?;
        Ok(())
    }

    /// Loads duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn load_duress_settings(&self) -> VauchiResult<Option<DuressSettings>> {
        Ok(self.storage.load_duress_settings()?)
    }

    /// Deletes duress alert settings.
    pub fn delete_duress_settings(&self) -> VauchiResult<()> {
        self.storage.delete_duress_settings()?;
        Ok(())
    }

    /// Returns a reference to the pending duress alerts queue.
    ///
    /// Alerts are queued when `authenticate()` detects a duress PIN.
    /// The sync system should drain this queue and send alerts as
    /// card updates to trusted contacts.
    pub fn pending_duress_alerts(&self) -> &[DuressAlert] {
        &self.duress_alerts
    }

    /// Queues a duress alert for sending to trusted contacts.
    ///
    /// Called internally by `authenticate()` when the duress PIN is entered.
    /// If no duress settings are configured, this is a no-op.
    ///
    /// The alert is stored in an in-memory queue. When the sync system
    /// connects, it drains this queue and sends alerts as card updates
    /// (indistinguishable from normal sync traffic).
    pub(super) fn queue_duress_alert(&mut self) -> VauchiResult<()> {
        let settings = self.storage.load_duress_settings()?;
        if let Some(_settings) = settings {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let device_id = self.device_id_string();

            let alert = DuressAlert {
                timestamp: now,
                device_id,
                alert_type: DuressAlertType::Unlock,
            };

            self.duress_alerts.push(alert);
        }
        Ok(())
    }

    /// Returns a string identifier for this device.
    ///
    /// Uses the identity's public ID if available, otherwise falls
    /// back to a placeholder. Used in duress alerts to identify the
    /// originating device.
    pub(super) fn device_id_string(&self) -> String {
        self.identity
            .as_ref()
            .map(|id| hex::encode(id.signing_public_key()))
            .unwrap_or_else(|| "unknown-device".to_string())
    }
}

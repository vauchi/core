// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery status, events, app password, duress PIN, and duress settings.

use std::time::{Duration, Instant};

use super::super::app_password::{AppPasswordConfig, AuthResult};
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::{EventCallback, VauchiEvent};
use super::{AuthMode, BiometricUnlockOutcome, Vauchi};
use crate::sleeper::Sleeper;
use crate::storage::ActivityLogRow;
use crate::types::DuressSettings;

/// Minimum wall-clock duration for [`Vauchi::biometric_unlock_check`].
///
/// Padding the call to a fixed floor hides whether a duress PIN is
/// configured: an observer cannot distinguish "biometric → straight
/// to ready" from "biometric → PIN screen for duress check" by
/// timing the unlock animation. iOS / Android previously enforced
/// this floor in their own code paths (see audit
/// `_private/docs/problems/2026-04-28-lifecycle-session-residue-umbrella`
/// item P2-B); core now owns the constant so the two language
/// implementations cannot drift apart.
pub const BIOMETRIC_UNLOCK_MIN_DURATION: Duration = Duration::from_millis(300);

/// Maximum number of contacts that can receive duress alerts.
///
/// Kept for settings validation even though the current duress flow triggers
/// a local wipe instead of sending alerts.
pub const MAX_DURESS_CONTACTS: usize = 5;

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
            .deliveries()
            .get_delivery_records_for_recipient(contact_id)?)
    }

    /// Gets all failed delivery records across all contacts.
    ///
    /// Returns delivery records with `Failed` status, useful for showing
    /// the user which messages need attention or retry.
    pub fn get_failed_deliveries(&self) -> VauchiResult<Vec<crate::storage::DeliveryRecord>> {
        Ok(self.storage.deliveries().get_delivery_records_by_status(
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
            .identity()
            .load_password_config()?
            .ok_or_else(|| VauchiError::InvalidState("no password configured".into()))?;

        match config.verify(password) {
            AuthResult::Normal => {
                self.auth_mode = AuthMode::Normal;
                Ok(AuthMode::Normal)
            }
            AuthResult::Duress => {
                self.auth_mode = AuthMode::Duress;
                // Queue covert duress alerts to configured trusted contacts.
                // Duress authentication itself must succeed even if alerting
                // fails, so errors are not propagated.
                if let Ok(Some(settings)) = self.load_duress_settings() {
                    let _count = self.queue_safety_alerts(
                        crate::sync::safety_alert::AlertKind::Duress,
                        &settings.alert_contact_ids,
                        &settings.alert_message,
                        None,
                    );
                }
                Ok(AuthMode::Duress)
            }
            AuthResult::Invalid => Err(VauchiError::InvalidState("invalid password".into())),
        }
    }

    /// Decides what to do after a successful platform biometric
    /// authentication, in constant wall-clock time.
    ///
    /// The frontend (iOS LAContext / Android BiometricPrompt) calls
    /// this immediately after the OS biometric prompt resolves with
    /// success. Returns:
    ///
    /// - [`BiometricUnlockOutcome::Unlocked`] when no duress PIN is
    ///   configured. `auth_mode` is set to [`AuthMode::Normal`] —
    ///   biometric proves the real user.
    /// - [`BiometricUnlockOutcome::PromptForDuressPin`] when duress is
    ///   configured. The frontend must show the PIN entry screen so
    ///   the user enters either the real PIN (-> `Normal` via
    ///   [`Vauchi::authenticate`]) or the duress PIN (-> `Duress`).
    ///   `auth_mode` is left untouched in this case.
    ///
    /// The call always takes at least
    /// [`BIOMETRIC_UNLOCK_MIN_DURATION`]. The
    /// `is_duress_enabled()` SQLite read is observably fast on most
    /// devices, so without padding the difference between the two
    /// outcomes would leak via the unlock-screen animation timing.
    /// Padding in core means iOS and Android cannot diverge on the
    /// floor (audit item
    /// `2026-04-28-lifecycle-session-residue-umbrella` P2-B).
    pub fn biometric_unlock_check(&mut self) -> VauchiResult<BiometricUnlockOutcome> {
        let start = self.monotonic.now();
        let outcome = self.biometric_unlock_decision()?;
        pad_to_minimum(
            self.sleeper.as_ref(),
            self.monotonic.as_ref(),
            start,
            BIOMETRIC_UNLOCK_MIN_DURATION,
        );
        Ok(outcome)
    }

    /// Inner decision logic for [`Self::biometric_unlock_check`] without
    /// the constant-time floor — exposed for tests so the assertion
    /// suite does not pay the 300 ms padding on every case.
    pub(crate) fn biometric_unlock_decision(&mut self) -> VauchiResult<BiometricUnlockOutcome> {
        if self.is_duress_enabled()? {
            Ok(BiometricUnlockOutcome::PromptForDuressPin)
        } else {
            self.auth_mode = AuthMode::Normal;
            Ok(BiometricUnlockOutcome::Unlocked)
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

        // Refuse to silently overwrite an existing credential: `setup_*`
        // creates the FIRST password; rotation must go through
        // `change_app_password` (which verifies the current one). Without
        // this guard a wrong-mode caller could replace the stored hash+salt
        // with no current-password check — a clobber/lockout risk
        // (2026-06-13-ios-app-password-setup-missing review).
        if self.storage.identity().load_password_config()?.is_some() {
            return Err(VauchiError::InvalidState(
                "app password already configured; use change_app_password to rotate".into(),
            ));
        }

        // Ensure the identity row exists in DB (may not yet if create_identity
        // only stored the own_card). Insert a placeholder row if missing.
        if !self.storage.identity().has_identity()? {
            self.storage.identity().save_identity(b"", "")?;
        }

        let config = AppPasswordConfig::create(password)?;
        self.storage
            .identity()
            .save_app_password(config.password_hash(), config.password_salt())?;

        Ok(())
    }

    /// Rotates the app password.
    ///
    /// Verifies `current_password` against the stored config — must
    /// match the **normal** password, not the duress PIN: a duress
    /// unlock is read-only decoy access and must never escalate to
    /// credential management. On a successful verify, generates a new
    /// salt + Argon2id hash for `new_password` (preserving any
    /// configured duress PIN byte-for-byte) and persists the rotated
    /// values to the identity table.
    ///
    /// On any failure (no identity, no existing config, wrong current,
    /// or new=duress collision) storage is **not** mutated, so the
    /// caller can retry with corrected input.
    pub fn change_app_password(
        &mut self,
        current_password: &str,
        new_password: &str,
    ) -> VauchiResult<()> {
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }

        let mut config = self
            .storage
            .identity()
            .load_password_config()?
            .ok_or_else(|| VauchiError::InvalidState("no password configured".into()))?;

        match config.verify(current_password) {
            AuthResult::Normal => {}
            AuthResult::Duress | AuthResult::Invalid => {
                return Err(VauchiError::InvalidState("invalid password".into()));
            }
        }

        config.change_password(new_password)?;

        self.storage
            .identity()
            .save_app_password(config.password_hash(), config.password_salt())?;

        Ok(())
    }

    /// Sets up a duress PIN.
    ///
    /// Requires an app password to be configured first.
    pub fn setup_duress_password(&mut self, duress_password: &str) -> VauchiResult<()> {
        let mut config = self
            .storage
            .identity()
            .load_password_config()?
            .ok_or_else(|| {
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
            .identity()
            .save_duress_password(duress_hash, duress_salt)?;

        Ok(())
    }

    /// Returns whether an app password has been configured.
    pub fn is_password_enabled(&self) -> VauchiResult<bool> {
        Ok(self.storage.identity().load_password_config()?.is_some())
    }

    /// Returns the activity log entries newer than `since_secs`.
    ///
    /// `now` is the caller's current timestamp so the query window is
    /// consistent with the caller's watermark bookkeeping (no redundant
    /// clock reads).
    pub fn activity_log_poll(
        &self,
        since_secs: u64,
        now: u64,
    ) -> VauchiResult<Vec<ActivityLogRow>> {
        let max_age = now.saturating_sub(since_secs);
        Ok(self
            .storage
            .activity_log()
            .activity_log_query_recent(now, max_age)?)
    }

    /// Returns whether duress mode is enabled.
    pub fn is_duress_enabled(&self) -> VauchiResult<bool> {
        match self.storage.identity().load_password_config()? {
            Some(config) => Ok(config.duress_enabled()),
            None => Ok(false),
        }
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&mut self) -> VauchiResult<()> {
        self.storage.identity().disable_duress()?;
        Ok(())
    }

    // === Duress Settings ===

    /// Saves duress alert settings (trusted contacts, message, location).
    pub fn save_duress_settings(&self, settings: &DuressSettings) -> VauchiResult<()> {
        if settings.alert_contact_ids.len() > MAX_DURESS_CONTACTS {
            return Err(VauchiError::InvalidState(format!(
                "maximum {MAX_DURESS_CONTACTS} duress contacts allowed, got {}",
                settings.alert_contact_ids.len()
            )));
        }
        self.storage.duress().save_duress_settings(settings)?;
        Ok(())
    }

    /// Loads duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn load_duress_settings(&self) -> VauchiResult<Option<DuressSettings>> {
        Ok(self.storage.duress().load_duress_settings()?)
    }

    /// Deletes duress alert settings.
    pub fn delete_duress_settings(&self) -> VauchiResult<()> {
        self.storage.duress().delete_duress_settings()?;
        Ok(())
    }

    /// Returns a string identifier for this device.
    ///
    /// Uses the identity's public ID if available, otherwise falls
    /// back to a placeholder.
    #[allow(dead_code)]
    pub(super) fn device_id_string(&self) -> String {
        self.identity
            .as_ref()
            .map(|id| hex::encode(id.signing_public_key()))
            .unwrap_or_else(|| "unknown-device".to_string())
    }
}

/// Sleep just long enough that the elapsed time since `start` is at
/// least `floor`. No-op when the elapsed time already meets or
/// exceeds the floor.
///
/// Phase 1 / Task 1.3 of the pure-functional-core program plan:
/// routes the suspension through the [`Sleeper`] seam so tests can
/// inject a `FakeSleeper` and avoid paying the 300 ms pad per call,
/// while still asserting the floor was requested. Production callers
/// pass `SystemSleeper` — the real wall-clock suspension required by
/// the constant-time invariant in
/// [`BIOMETRIC_UNLOCK_MIN_DURATION`]'s docs.
fn pad_to_minimum(
    sleeper: &dyn Sleeper,
    monotonic: &dyn crate::monotonic::MonotonicClock,
    start: Instant,
    floor: Duration,
) {
    let now = monotonic.now();
    let elapsed = now.duration_since(start);
    if elapsed < floor {
        sleeper.sleep(floor - elapsed);
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress lock policy (ADR-032, amended 2026-11-10): a duress
//! credential needs biometric unlock in front of a 6-digit PIN, or a real
//! password for both the app password and the duress credential.

use crate::api::app_password::AuthResult;
use crate::api::{VauchiError, VauchiResult};
use crate::emergency::{DuressUnlock, WeakCredential};

use super::ProfileEngine;

/// Holds `password` to the backup password's strength rule, naming
/// `which` credential fell short.
pub(super) fn require_real_password(password: &str, which: WeakCredential) -> VauchiResult<()> {
    crate::identity::password::validate_password(password)
        .map(|_| ())
        .map_err(|_| VauchiError::WeakCredential(which))
}

impl ProfileEngine {
    /// The old name of [`Self::setup_duress_pin_with_biometric_unlock`],
    /// kept so consumers pinned to an older Core build until their next
    /// bump moves them to the new name (e2e's smoke builds against this
    /// branch).
    #[deprecated(note = "use setup_duress_pin_with_biometric_unlock")]
    pub fn setup_duress_password(&mut self, duress_password: &str) -> VauchiResult<()> {
        self.setup_duress_pin_with_biometric_unlock(duress_password)
    }

    /// Sets up a duress credential under the password branch. Core keeps
    /// only the app password's hash, so the caller passes the app password
    /// itself: it must open the profile in front, and both it and
    /// `duress_password` must pass the backup password's strength rule.
    pub fn setup_duress_password_with_app_password(
        &mut self,
        app_password: &str,
        duress_password: &str,
    ) -> VauchiResult<()> {
        let config = self.password_config()?.ok_or_else(|| {
            VauchiError::InvalidState("app password must be set before duress PIN".into())
        })?;
        let opens_the_profile_in_front = match config.verify(app_password) {
            AuthResult::Normal => !self.in_duress_mode(),
            AuthResult::Duress => self.in_duress_mode(),
            AuthResult::Invalid => false,
        };
        if !opens_the_profile_in_front {
            return Err(VauchiError::InvalidState("invalid password".into()));
        }
        require_real_password(app_password, WeakCredential::AppPassword)?;
        require_real_password(duress_password, WeakCredential::DuressPassword)?;
        self.save_duress_credential(duress_password, DuressUnlock::Passwords)
    }

    /// The branch duress was set up under, or `None` without duress. Under
    /// duress this is the concealed view (#462).
    pub fn duress_unlock(&self) -> VauchiResult<Option<DuressUnlock>> {
        if self.in_duress_mode() {
            let concealed = self.concealed_duress();
            return Ok(concealed.enabled.then_some(concealed.unlock));
        }
        Ok(self.storage.identity().duress_unlock()?)
    }
}

impl super::Vauchi {
    /// The lock screen's unlock. Under the biometric branch the PIN counts
    /// only after a biometric pass on a device that has biometrics. Every
    /// install with biometric unlock shows the same lock screen, so a PIN
    /// typed before the pass is refused as a wrong one, after the same
    /// Argon2id work, and neither the answer nor its time shows that duress
    /// is set. The real setup decides, as for
    /// [`Self::biometric_unlock_check`], so a lock after a duress session
    /// behaves the same.
    pub fn authenticate_at_lock(
        &mut self,
        password: &str,
        biometric_passed: bool,
        device_has_biometrics: bool,
    ) -> VauchiResult<super::AuthMode> {
        let real = self.real_engine_mut();
        let waits_for_the_pass = device_has_biometrics
            && !biometric_passed
            && real.storage.identity().duress_unlock()? == Some(DuressUnlock::Biometric);
        if waits_for_the_pass {
            if let Some(config) = real.password_config()? {
                let _same_work_as_a_wrong_pin = config.verify(password);
            }
            return Err(VauchiError::InvalidState("invalid password".into()));
        }
        self.authenticate(password)
    }
}

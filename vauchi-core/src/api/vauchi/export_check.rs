// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The opt-in biometric check before a backup export (ADR-080 point 7,
//! amended 2026-11-09).

use crate::api::VauchiResult;

use super::ProfileEngine;

impl ProfileEngine {
    /// Whether a backup export asks for a biometric check before the
    /// password.
    pub fn confirms_exports_with_biometrics(&self) -> VauchiResult<bool> {
        Ok(self.load_settings_flags()?.confirm_exports_with_biometrics)
    }

    /// Turns the biometric check before a backup export on or off. On the
    /// real profile the mask gets the same setting, so an export under
    /// duress shows the same step; under duress only the mask changes.
    pub fn set_confirm_exports_with_biometrics(&self, on: bool) -> VauchiResult<()> {
        let mut flags = self.load_settings_flags()?;
        flags.confirm_exports_with_biometrics = on;
        self.save_settings_flags(&flags)?;
        if !self.is_mask() {
            let mask = self.storage.mask_profile();
            let mut mask_flags = mask.ux().load_settings_flags()?.unwrap_or_default();
            mask_flags.confirm_exports_with_biometrics = on;
            mask.ux().save_settings_flags(&mask_flags)?;
            self.storage.seal_mask()?;
        }
        Ok(())
    }
}

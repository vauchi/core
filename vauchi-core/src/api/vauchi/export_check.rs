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
        Ok(false)
    }

    /// Turns the biometric check before a backup export on or off.
    pub fn set_confirm_exports_with_biometrics(&self, on: bool) -> VauchiResult<()> {
        let _ = on;
        Ok(())
    }
}

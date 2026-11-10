// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress lock policy (ADR-032, amended 2026-11-10): a duress
//! credential needs biometric unlock in front of a 6-digit PIN, or a real
//! password for both the app password and the duress credential.

use crate::api::{VauchiError, VauchiResult};
use crate::emergency::DuressUnlock;

use super::ProfileEngine;

impl ProfileEngine {
    /// Sets up a duress credential under the password branch.
    pub fn setup_duress_password_with_app_password(
        &mut self,
        _app_password: &str,
        _duress_password: &str,
    ) -> VauchiResult<()> {
        Err(VauchiError::InvalidState("not built yet".into()))
    }

    /// The branch duress was set up under, or `None` without duress.
    pub fn duress_unlock(&self) -> VauchiResult<Option<DuressUnlock>> {
        Ok(None)
    }
}

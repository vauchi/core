// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The personal-data (GDPR Art. 15) export, following the auth mode.

use super::super::error::VauchiResult;
use super::ProfileEngine;
use crate::api::gdpr::{GdprExport, encrypt_export, export_all_data};

impl ProfileEngine {
    /// Exports the user's personal data: under duress, the mask profile's
    /// (ADR-080 point 4).
    pub fn export_personal_data(&self) -> VauchiResult<GdprExport> {
        Ok(export_all_data(&self.storage)?)
    }

    /// [`ProfileEngine::export_personal_data`], encrypted under `password`. The free
    /// `export_encrypted(storage, ...)` reads the real store whatever the
    /// auth mode; this follows it (private#616).
    pub fn export_personal_data_encrypted(&self, password: &str) -> VauchiResult<Vec<u8>> {
        Ok(encrypt_export(&self.export_personal_data()?, password)?)
    }
}

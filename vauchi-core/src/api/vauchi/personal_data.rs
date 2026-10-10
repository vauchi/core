// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The personal-data (GDPR Art. 15) export, following the auth mode.

use super::super::error::VauchiResult;
use super::{AuthMode, Vauchi};
use crate::api::gdpr::{GdprExport, encrypt_export, export_all_data, export_for_contacts};

impl Vauchi {
    /// Exports the user's personal data. Like every other read it follows
    /// the active auth mode: the decoy profile exports its decoy contacts
    /// and no audit log (ADR-032).
    pub fn export_personal_data(&self) -> VauchiResult<GdprExport> {
        if self.auth_mode == AuthMode::Duress {
            let decoys = self.list_contacts()?;
            return Ok(export_for_contacts(&self.storage, &decoys, false)?);
        }
        Ok(export_all_data(&self.storage)?)
    }

    /// [`Vauchi::export_personal_data`], encrypted under `password`. The free
    /// `export_encrypted(storage, ...)` reads the real store whatever the
    /// auth mode; this follows it (private#616).
    pub fn export_personal_data_encrypted(&self, password: &str) -> VauchiResult<Vec<u8>> {
        Ok(encrypt_export(&self.export_personal_data()?, password)?)
    }
}

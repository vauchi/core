// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact import operations — vCard file import for mobile platforms.

use super::VauchiPlatform;
use super::error::MobileError;

/// Result of a contact import operation.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileImportResult {
    /// Number of contacts successfully imported.
    pub imported: u32,
    /// Number of contacts skipped (malformed or duplicate).
    pub skipped: u32,
    /// Warning messages for skipped contacts.
    pub warnings: Vec<String>,
}

#[uniffi::export]
impl VauchiPlatform {
    /// Import contacts from vCard data (supports 2.1 / 3.0 / 4.0).
    ///
    /// Pass the raw bytes of a `.vcf` file. Each parsed vCard becomes an
    /// imported contact. Duplicates (by UID) are skipped. Returns counts
    /// and per-contact warnings.
    pub fn import_contacts_from_vcf(
        &self,
        data: Vec<u8>,
    ) -> Result<MobileImportResult, MobileError> {
        let vauchi = self.open_vauchi()?;
        let result = vauchi
            .import_contacts_from_vcf(&data)
            .map_err(|e| MobileError::Other {
                message: e.to_string(),
            })?;
        Ok(MobileImportResult {
            imported: result.imported as u32,
            skipped: result.skipped as u32,
            warnings: result.warnings,
        })
    }
}

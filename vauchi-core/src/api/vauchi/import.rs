// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact import API — wraps the vCard parser and persists imported contacts.

use crate::ImportSource;
use crate::contact::Contact;
use crate::contact_card::vcard_import::import_vcf;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// Result of a contact import operation.
pub struct ImportResult {
    /// Number of contacts successfully imported.
    pub imported: usize,
    /// Number of contacts skipped (malformed or duplicate).
    pub skipped: usize,
    /// Warning messages for skipped contacts.
    pub warnings: Vec<String>,
}

impl Vauchi {
    /// Imports contacts from vCard data (supports 2.1 / 3.0 / 4.0, multi-contact files).
    ///
    /// Each successfully parsed vCard becomes an imported [`Contact`] stored
    /// with [`ImportSource::VcardFile`].  Cards that fail to save (e.g. because
    /// storage is full) are counted as skipped with a warning message.
    ///
    /// Returns an [`ImportResult`] with counts and any per-contact warnings.
    pub fn import_contacts_from_vcf(&self, data: &[u8]) -> VauchiResult<ImportResult> {
        let entries = import_vcf(data).map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        let mut imported = 0;
        let mut skipped = 0;
        let mut warnings = Vec::new();

        for (card, uid) in entries {
            let contact = Contact::from_import(card, ImportSource::VcardFile, uid);
            match self.storage.save_contact(&contact) {
                Ok(_) => imported += 1,
                Err(e) => {
                    warnings.push(format!("Skipped contact: {}", e));
                    skipped += 1;
                }
            }
        }

        Ok(ImportResult {
            imported,
            skipped,
            warnings,
        })
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact import API — wraps the vCard parser and persists imported contacts.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ImportSource;
use crate::contact::Contact;
use crate::contact_card::vcard_import::import_vcf;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// Reason a single vCard entry was skipped (G6 of the pure-renderer
/// remediation — ADR tracks at `_private/docs/problems/2026-04-16-frontend-pure-renderer-violations/`).
///
/// Each variant carries the data frontends need to render a localized
/// message; the `Display` impl produces the English rendering used by
/// CLI + tests + unlocalized fallback paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportWarning {
    /// A vCard with this UID already exists and was skipped (W7).
    DuplicateUid { uid: String },
    /// The per-identity contact limit was reached; remaining vCards
    /// were not imported. `max` is the configured limit (C3).
    ContactLimitReached { max: usize },
    /// Storage rejected the contact (disk full, schema failure, etc.).
    SaveFailed { error: String },
}

impl ImportWarning {
    /// Stable i18n key frontends look up via their `t()` helper.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            ImportWarning::DuplicateUid { .. } => "import.warning.duplicate_uid",
            ImportWarning::ContactLimitReached { .. } => "import.warning.limit_reached",
            ImportWarning::SaveFailed { .. } => "import.warning.save_failed",
        }
    }

    /// Placeholder values for the i18n template (e.g. `{"uid" => "abc"}`).
    pub fn args(&self) -> Vec<(String, String)> {
        match self {
            ImportWarning::DuplicateUid { uid } => vec![("uid".into(), uid.clone())],
            ImportWarning::ContactLimitReached { max } => vec![("max".into(), max.to_string())],
            ImportWarning::SaveFailed { error } => vec![("error".into(), error.clone())],
        }
    }
}

impl fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportWarning::DuplicateUid { uid } => write!(f, "Skipped duplicate (UID: {uid})"),
            ImportWarning::ContactLimitReached { max } => write!(
                f,
                "Contact limit reached ({max}); skipped remaining imports"
            ),
            ImportWarning::SaveFailed { error } => write!(f, "Skipped contact: {error}"),
        }
    }
}

/// Result of a contact import operation.
pub struct ImportResult {
    /// Number of contacts successfully imported.
    pub imported: usize,
    /// Number of contacts skipped (malformed or duplicate).
    pub skipped: usize,
    /// Structured per-contact warnings. Call `.to_string()` / `{}` for
    /// the English rendering, or read `i18n_key()` + `args()` for
    /// localized display.
    pub warnings: Vec<ImportWarning>,
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

        // C3: Enforce contact limit — compute remaining budget once before the loop.
        let current_count = self.storage.count_contacts()?;
        let max_contacts = self.storage.get_contact_limit()?;
        let mut remaining_budget = max_contacts.saturating_sub(current_count);

        for (card, uid) in entries {
            // W7: Skip if a contact with this original_uid already exists.
            if let Some(ref uid_val) = uid
                && self.storage.find_imported_by_uid(uid_val)?.is_some()
            {
                skipped += 1;
                warnings.push(ImportWarning::DuplicateUid {
                    uid: uid_val.clone(),
                });
                continue;
            }

            // C3: Stop importing when budget exhausted.
            if remaining_budget == 0 {
                skipped += 1;
                warnings.push(ImportWarning::ContactLimitReached { max: max_contacts });
                // Count the rest as skipped without iterating one-by-one.
                // We already incremented skipped for this entry; the loop
                // will naturally stop producing more warnings.
                continue;
            }

            let contact = Contact::from_import(card, ImportSource::VcardFile, uid);
            match self.storage.save_contact(&contact) {
                Ok(_) => {
                    imported += 1;
                    remaining_budget = remaining_budget.saturating_sub(1);
                }
                Err(e) => {
                    warnings.push(ImportWarning::SaveFailed {
                        error: e.to_string(),
                    });
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

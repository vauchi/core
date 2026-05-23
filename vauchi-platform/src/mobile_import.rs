// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact import operations — vCard file import for mobile platforms.

use vauchi_core::api::ImportWarning;

/// One localized-ready warning from a vCard import (G6 of the
/// pure-renderer remediation). Frontends look up `key` in their
/// localization store and substitute `args`; `legacy_text` keeps the
/// English rendering for log paths and unmigrated display code.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileImportWarning {
    /// Stable i18n key, e.g. `"import.warning.duplicate_uid"`.
    pub key: String,
    /// Placeholder values (e.g. `{"uid": "abc123"}`).
    pub args: std::collections::HashMap<String, String>,
    /// Pre-rendered English text. Safe to display verbatim.
    pub legacy_text: String,
}

impl From<ImportWarning> for MobileImportWarning {
    fn from(warning: ImportWarning) -> Self {
        let legacy_text = warning.to_string();
        let key = warning.i18n_key().to_string();
        let args: std::collections::HashMap<String, String> = warning.args().into_iter().collect();
        MobileImportWarning {
            key,
            args,
            legacy_text,
        }
    }
}

/// Result of a contact import operation.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileImportResult {
    /// Number of contacts successfully imported.
    pub imported: u32,
    /// Number of contacts skipped (malformed or duplicate).
    pub skipped: u32,
    /// Structured per-contact warnings. Frontends localize via
    /// `t(warning.key, warning.args)`; callers that do not yet
    /// localize can display `warning.legacy_text` verbatim.
    pub warnings: Vec<MobileImportWarning>,
}

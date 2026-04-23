// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! G6 import-warning serialization helper used by
//! `vauchi_app_import_contacts_from_vcf`. Kept in its own file so
//! `app.rs` stays under the file-size threshold while retaining inline
//! tests (the `cdylib` crate-type prevents integration tests in a
//! `tests/` directory).

use vauchi_core::api::ImportWarning;

/// Serialize a slice of `ImportWarning` into the G6 `{key, args,
/// legacy_text}` JSON shape (matching UniFFI's `MobileImportWarning`
/// record). Consumers look `key` up via `i18n_get_string`, substitute
/// `args`, or fall back to `legacy_text` when the key isn't localized.
pub(super) fn warnings_to_json(warnings: &[ImportWarning]) -> Vec<serde_json::Value> {
    warnings
        .iter()
        .map(|w| {
            let args: std::collections::BTreeMap<String, String> = w.args().into_iter().collect();
            serde_json::json!({
                "key": w.i18n_key(),
                "args": args,
                "legacy_text": w.to_string(),
            })
        })
        .collect()
}

// INLINE_TEST_REQUIRED: cdylib crate-type prevents integration tests in tests/ directory
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn duplicate_uid_serializes_with_key_args_legacy_text() {
        let warnings = vec![ImportWarning::DuplicateUid {
            uid: "abc123".into(),
        }];
        let json = warnings_to_json(&warnings);
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["key"], "import.warning.duplicate_uid");
        assert_eq!(json[0]["args"]["uid"], "abc123");
        assert_eq!(json[0]["legacy_text"], "Skipped duplicate (UID: abc123)");
    }

    // @internal
    #[test]
    fn contact_limit_reached_includes_max_arg() {
        let warnings = vec![ImportWarning::ContactLimitReached { max: 500 }];
        let json = warnings_to_json(&warnings);
        assert_eq!(json[0]["key"], "import.warning.limit_reached");
        assert_eq!(json[0]["args"]["max"], "500");
        assert_eq!(
            json[0]["legacy_text"],
            "Contact limit reached (500); skipped remaining imports"
        );
    }

    // @internal
    #[test]
    fn save_failed_includes_error_arg() {
        let warnings = vec![ImportWarning::SaveFailed {
            error: "disk full".into(),
        }];
        let json = warnings_to_json(&warnings);
        assert_eq!(json[0]["key"], "import.warning.save_failed");
        assert_eq!(json[0]["args"]["error"], "disk full");
    }

    // @internal
    #[test]
    fn empty_warnings_produces_empty_array() {
        let json = warnings_to_json(&[]);
        assert!(json.is_empty());
    }

    // @internal
    #[test]
    fn multiple_warnings_preserve_order() {
        let warnings = vec![
            ImportWarning::DuplicateUid {
                uid: "first".into(),
            },
            ImportWarning::ContactLimitReached { max: 100 },
        ];
        let json = warnings_to_json(&warnings);
        assert_eq!(json.len(), 2);
        assert_eq!(json[0]["args"]["uid"], "first");
        assert_eq!(json[1]["args"]["max"], "100");
    }
}

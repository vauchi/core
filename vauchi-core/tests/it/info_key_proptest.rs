// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Verifies that every info_key used in the codebase resolves to non-empty locale content.

#[test]
fn all_info_keys_resolve_to_content() {
    let known_keys = [
        "groups_purpose",
        "visibility_meaning",
        "contact_info_optional",
        "auto_lock",
        "duress_pin",
        "recovery_trust",
        "backup_password",
    ];
    for key in &known_keys {
        let result =
            vauchi_app::ui::info_content::resolve_info_key(key, vauchi_app::i18n::Locale::English);
        assert!(result.is_some(), "info_key '{key}' has no locale content");
        let (title, body) = result.unwrap();
        assert!(!title.is_empty(), "info_key '{key}' has empty title");
        assert!(!body.is_empty(), "info_key '{key}' has empty body");
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core resolves the display copy for a wire `Field`'s visibility state
//! (`visibility_label`); frontends render it verbatim and never derive
//! copy from the `UiFieldVisibility` discriminant (ADR-043/044,
//! `2026-07-06-desktop-tui-web-domain-shell-violations`).

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{UiFieldVisibility, visibility_label};

fn load_locale(code: &str, file: &str) {
    let bytes = std::fs::read(crate::i18n_support::locales_dir().join(file))
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes(code, &bytes).expect("locale parses");
}

// @internal
#[test]
fn shown_and_hidden_resolve_to_localized_labels() {
    load_locale("en", "en.json");
    assert_eq!(
        visibility_label(&UiFieldVisibility::Shown, Locale::ENGLISH),
        "Visible"
    );
    assert_eq!(
        visibility_label(&UiFieldVisibility::Hidden, Locale::ENGLISH),
        "Hidden"
    );
}

// @internal
#[test]
fn empty_scopes_resolve_to_no_groups_label() {
    load_locale("en", "en.json");
    assert_eq!(
        visibility_label(&UiFieldVisibility::Scopes(vec![]), Locale::ENGLISH),
        "No groups"
    );
}

// @internal
#[test]
fn named_scopes_resolve_to_joined_names() {
    let scopes = vec!["Family".to_string(), "Work".to_string()];
    assert_eq!(
        visibility_label(&UiFieldVisibility::Scopes(scopes), Locale::ENGLISH),
        "Family, Work"
    );
}

// @internal
#[test]
fn german_labels_resolve_from_the_active_locale() {
    load_locale("de", "de.json");
    assert_eq!(
        visibility_label(&UiFieldVisibility::Hidden, Locale::GERMAN),
        "Verborgen"
    );
    assert_eq!(
        visibility_label(&UiFieldVisibility::Scopes(vec![]), Locale::GERMAN),
        "Keine Gruppen"
    );
}

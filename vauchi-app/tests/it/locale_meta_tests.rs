// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A language Core has no table entry for is named by its own file's
//! `_meta`, so a CDN language shows under its own name; a `_meta` that
//! disagrees with its file name or carries an unusable name keeps the
//! file's strings but names nothing (vauchi/private#733 D2). Each test
//! uses its own language code because the string store is global.

use vauchi_app::i18n::{Locale, get_locale_info, get_string, load_locale_from_bytes};

fn locale(code: &str) -> Locale {
    Locale::from_code(code).unwrap()
}

// @internal
#[test]
fn a_loaded_files_meta_names_its_language() {
    load_locale_from_bytes(
        "pt",
        r#"{"_meta":{"locale":"pt","name":"Português","english_name":"Portuguese","is_rtl":false},"t.k":"olá"}"#
            .as_bytes(),
    )
    .unwrap();

    let info = get_locale_info(locale("pt"));

    assert_eq!(info.code, "pt");
    assert_eq!(info.name, "Português");
    assert_eq!(info.english_name, "Portuguese");
    assert!(!info.is_rtl);
}

// @internal
#[test]
fn a_right_to_left_language_says_so() {
    load_locale_from_bytes(
        "fa",
        r#"{"_meta":{"locale":"fa","name":"فارسی","english_name":"Persian","is_rtl":true}}"#
            .as_bytes(),
    )
    .unwrap();

    let info = get_locale_info(locale("fa"));

    assert_eq!(info.name, "فارسی");
    assert!(info.is_rtl);
}

// @internal
#[test]
fn a_language_without_a_loaded_file_is_named_by_its_code() {
    let info = get_locale_info(locale("sw"));

    assert_eq!(info.code, "sw");
    assert_eq!(info.name, "sw");
    assert_eq!(info.english_name, "sw");
    assert!(!info.is_rtl);
}

// @internal
#[test]
fn an_unusable_meta_keeps_the_strings_but_names_nothing() {
    let long_name = "n".repeat(65);
    let cases = [
        (
            "nl",
            r#"{"locale":"de","name":"Nederlands","english_name":"Dutch","is_rtl":false}"#
                .to_string(),
        ),
        (
            "sv",
            format!(
                r#"{{"locale":"sv","name":"{long_name}","english_name":"Swedish","is_rtl":false}}"#
            ),
        ),
        (
            "fi",
            r#"{"locale":"fi","name":"","english_name":"Finnish","is_rtl":false}"#.to_string(),
        ),
        (
            "pl",
            r#"{"locale":"pl","name":"Pol\u0000ski","english_name":"Polish","is_rtl":false}"#
                .to_string(),
        ),
        (
            "cs",
            r#"{"locale":"cs","name":"Čeština","english_name":"Czech\nInjected","is_rtl":false}"#
                .to_string(),
        ),
        (
            "da",
            r#"{"locale":"da","english_name":"Danish","is_rtl":false}"#.to_string(),
        ),
        ("hu", r#""magyar""#.to_string()),
    ];
    for (code, meta) in cases {
        let file = format!(r#"{{"_meta":{meta},"t.k":"value-{code}"}}"#);
        load_locale_from_bytes(code, file.as_bytes()).unwrap();

        assert_eq!(
            get_string(locale(code), "t.k"),
            format!("value-{code}"),
            "{code}"
        );
        assert_eq!(get_locale_info(locale(code)).name, code, "{code}");
    }
}

// @internal
#[test]
fn a_reloaded_file_with_an_unusable_meta_drops_the_earlier_name() {
    load_locale_from_bytes(
        "bg",
        r#"{"_meta":{"locale":"bg","name":"Български","english_name":"Bulgarian","is_rtl":false}}"#
            .as_bytes(),
    )
    .unwrap();
    load_locale_from_bytes("bg", r#"{"_meta":{"locale":"xx"}}"#.as_bytes()).unwrap();

    assert_eq!(get_locale_info(locale("bg")).name, "bg");
}

/// Names come only from data (vauchi/private#733 G2): a bundled language
/// whose file has not loaded is named by its code like any other, and
/// English by the `_meta` of the copy Core embeds.
// @internal
#[test]
fn no_language_is_named_by_a_table_in_code() {
    let german = get_locale_info(Locale::GERMAN);
    assert_eq!(
        (german.name.as_str(), german.english_name.as_str()),
        ("de", "de")
    );

    let english = get_locale_info(Locale::ENGLISH);
    assert_eq!(
        (english.name.as_str(), english.english_name.as_str()),
        ("English", "English")
    );
}

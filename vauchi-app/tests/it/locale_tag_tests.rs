// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A language published on the CDN must be selectable without a Core
//! release, so `Locale` holds any well-formed language subtag instead of
//! one of five compiled-in variants (vauchi/private#733 D1).

use proptest::prelude::*;
use vauchi_app::i18n::Locale;

// @internal
#[test]
fn a_language_core_has_no_variant_for_parses_to_its_subtag() {
    for (tag, code) in [
        ("pt", "pt"),
        ("pt-BR", "pt"),
        ("PT_br.UTF-8", "pt"),
        ("fil", "fil"),
        ("fil-PH", "fil"),
    ] {
        assert_eq!(
            Locale::from_code(tag).map(|l| l.code().to_string()),
            Some(code.to_string()),
            "tag {tag:?}"
        );
    }
}

// @internal
#[test]
fn the_bundled_constants_carry_their_codes() {
    for (locale, code) in [
        (Locale::ENGLISH, "en"),
        (Locale::GERMAN, "de"),
        (Locale::FRENCH, "fr"),
        (Locale::SPANISH, "es"),
        (Locale::ITALIAN, "it"),
    ] {
        assert_eq!(locale.code(), code);
        assert_eq!(Locale::from_code(code), Some(locale));
    }
    assert_eq!(Locale::default(), Locale::ENGLISH);
}

// @internal
#[test]
fn a_malformed_language_subtag_is_rejected() {
    let long = "a".repeat(1024);
    let long_with_valid_subtag = format!("en-{}", "x".repeat(70));
    for tag in [
        "",
        "-",
        "e",
        "engl",
        "en\0de",
        "\0en",
        "é",
        "ée",
        "ен",
        "e1",
        "1en",
        "en ",
        " en",
        "../en",
        long.as_str(),
        long_with_valid_subtag.as_str(),
    ] {
        assert_eq!(Locale::from_code(tag), None, "tag {tag:?}");
    }
}

// @internal
#[test]
fn a_locale_serializes_as_its_code_and_rejects_a_malformed_one() {
    let pt = Locale::from_code("pt").unwrap();
    assert_eq!(serde_json::to_string(&pt).unwrap(), "\"pt\"");
    assert_eq!(serde_json::from_str::<Locale>("\"pt\"").unwrap(), pt);
    assert_eq!(
        serde_json::from_str::<Locale>("\"de\"").unwrap(),
        Locale::GERMAN
    );
    assert!(serde_json::from_str::<Locale>("\"engl\"").is_err());
    assert!(serde_json::from_str::<Locale>("\"\"").is_err());
}

proptest! {
    // @internal
    #[test]
    fn every_parsed_locale_round_trips_through_its_code(tag in "[a-zA-Z]{1,4}([-_.][a-zA-Z0-9]{0,8})?") {
        if let Some(locale) = Locale::from_code(&tag) {
            prop_assert_eq!(Locale::from_code(locale.code()), Some(locale));
            let language = tag.split(['-', '_', '.']).next().unwrap().to_ascii_lowercase();
            prop_assert_eq!(locale.code(), language.as_str());
        } else {
            let language = tag.split(['-', '_', '.']).next().unwrap();
            prop_assert!(!(2..=3).contains(&language.len()));
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bindings carry language codes, not a five-variant enum, so a
//! language published on the CDN reaches every shell without a binding
//! change (vauchi/private#733 D5). The string store is process-global;
//! nextest runs each test in its own process.

use std::collections::HashMap;
use vauchi_platform::{get_locale_info, get_string, get_string_with_args, parse_locale_code};

const PORTUGUESE: &str = r#"{
    "_meta": {"locale": "pt", "name": "Português", "english_name": "Portuguese", "is_rtl": false},
    "greeting": "Olá {name}"
}"#;

// @internal
#[test]
fn a_tag_parses_to_its_language_code() {
    assert_eq!(parse_locale_code("de-DE".into()), Some("de".to_string()));
    assert_eq!(parse_locale_code("pt-BR".into()), Some("pt".to_string()));
    assert_eq!(parse_locale_code("engl".into()), None);
    assert_eq!(parse_locale_code(String::new()), None);
}

// @internal
#[test]
fn strings_and_info_are_read_by_code() {
    vauchi_app::i18n::load_locale_from_bytes("pt", PORTUGUESE.as_bytes()).unwrap();
    let args = HashMap::from([("name".to_string(), "Ana".to_string())]);

    assert_eq!(
        get_string_with_args("pt".into(), "greeting".into(), args),
        "Olá Ana"
    );
    let info = get_locale_info("pt".into());
    assert_eq!(
        (
            info.code.as_str(),
            info.name.as_str(),
            info.english_name.as_str()
        ),
        ("pt", "Português", "Portuguese")
    );
}

// @internal
#[test]
fn a_malformed_code_reads_english() {
    let english = get_string("en".into(), "contacts.title".into());

    assert_eq!(get_string("x\0y".into(), "contacts.title".into()), english);
    assert_eq!(get_locale_info("x\0y".into()).code, "en");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The bundled FAQ catalog answers the questions users ask first, in
//! every language the app ships.

use super::i18n_support::{assert_translated, load_locale};
use vauchi_app::help::{get_faq_by_id, get_faq_by_id_localized};
use vauchi_app::i18n::Locale;

// @scenario: help_faq :: FAQ content covers core topics
#[test]
fn every_core_topic_has_an_faq_with_its_copy() {
    let topics = [
        ("What happens if I lose my phone", "faq-phone-lost"),
        ("How to set up recovery", "faq-recovery-setup"),
        ("Does Vauchi track me", "faq-tracking"),
        ("Where is my data stored", "faq-data-storage"),
        ("How is my data encrypted", "faq-encryption"),
        ("How to remove a contact", "faq-remove-contact"),
        ("How to block a contact", "faq-block-contact"),
        ("How updates work", "faq-how-updates-work"),
        ("What happens when I'm offline", "faq-offline-updates"),
        ("How to add my first contact", "faq-first-contact"),
        ("Why exchange is in-person", "faq-why-in-person"),
        ("What are visibility labels", "faq-visibility-labels"),
        ("Default visibility rules", "faq-visibility-default"),
        ("Using multiple devices", "faq-multiple-devices"),
    ];

    for (topic, id) in topics {
        let faq = get_faq_by_id(id).unwrap_or_else(|| panic!("{topic}: no {id}"));
        for text in [&faq.question, &faq.answer] {
            assert!(
                !text.is_empty() && !text.starts_with("Missing:"),
                "{topic}: {id} has no English copy: {faq:?}"
            );
        }
    }
}

// @scenario: help_faq :: FAQ localization for supported languages
#[test]
fn the_tracking_faq_is_translated_into_german_french_and_spanish() {
    let english = get_faq_by_id_localized("faq-tracking", Locale::ENGLISH).unwrap();
    assert_eq!(english.question, "Can someone track me through Vauchi?");

    for (code, locale) in [
        ("de", Locale::GERMAN),
        ("fr", Locale::FRENCH),
        ("es", Locale::SPANISH),
    ] {
        load_locale(code);
        let faq = get_faq_by_id_localized("faq-tracking", locale).unwrap();
        assert_translated(code, &faq.question, &english.question);
        assert_translated(code, &faq.answer, &english.answer);
    }
}

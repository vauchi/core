// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the third batch of screens
//! renders in the user's locale. Per-screen a11y/copy detail stays in each
//! engine's own inline unit tests.
//!
//! Asserts that each screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. The screens
//! are checked as one table so adding an engine is a one-line change; the
//! failure message names the screen.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ArchivedContactsEngine, ChangePasswordEngine, ContactVisibilityEngine, DecoyContactsEngine,
    DeepLinkConsentEngine, DuplicateDetectionEngine, FingerprintVerifyEngine, GroupInfo,
    GroupsEngine, GroupsMode, HelpEngine, LinkResponderEngine, LockScreenEngine, PlacesEngine,
    SupportEngine, TagPromotionEngine, WorkflowEngine,
};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

fn deep_link_payload() -> vauchi_core::exchange::link_mode::DeepLinkPayload {
    let (init, _) = initiator_generate();
    parse_exchange_deep_link(&init.url).expect("canonical URL parses")
}

fn sample_groups() -> Vec<GroupInfo> {
    vec![GroupInfo {
        id: "g1".into(),
        name: "Work".into(),
        member_count: 1,
        visible_field_count: 1,
    }]
}

/// The groups-list view-mode toggle label.
fn groups_view_mode_label(locale: Locale) -> String {
    GroupsEngine::new(sample_groups(), GroupsMode::Members)
        .with_locale(locale)
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::ToggleList { label, .. } => Some(label.clone()),
            _ => None,
        })
        .expect("groups list renders a ToggleList")
}

/// Every screen title in this batch, for one locale.
fn screen_titles(locale: Locale) -> Vec<(&'static str, String)> {
    let title = |s: vauchi_app::ui::ScreenModel| s.title;
    vec![
        (
            "archived contacts",
            title(
                ArchivedContactsEngine::new(vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "contact visibility",
            title(
                ContactVisibilityEngine::new("Alice".into(), vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "activity log",
            title(
                vauchi_app::ui::ActivityLogEngine::new(vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "lock screen",
            title(
                LockScreenEngine::new(5)
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "tag promotion",
            title(
                TagPromotionEngine::new("t1".into(), "Friends".into(), 3, vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "duplicate detection",
            title(
                DuplicateDetectionEngine::new(vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "decoy contacts",
            title(
                DecoyContactsEngine::new(vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "deep-link consent",
            title(
                DeepLinkConsentEngine::new(deep_link_payload())
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "link responder",
            title(
                LinkResponderEngine::new(deep_link_payload())
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "change password",
            title(
                ChangePasswordEngine::new(true)
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "fingerprint verify",
            title(
                FingerprintVerifyEngine::new("c1", "AA:BB", "CC:DD", false)
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "places list",
            title(
                PlacesEngine::new(vec![])
                    .with_locale(locale)
                    .current_screen(),
            ),
        ),
        (
            "help",
            title(HelpEngine::new(vec![]).with_locale(locale).current_screen()),
        ),
        (
            "support",
            title(SupportEngine::new().with_locale(locale).current_screen()),
        ),
    ]
}

// @scenario: batch3 :: every screen renders in the active locale
// @internal
#[test]
fn batch3_screens_render_the_active_locale() {
    load_german();
    let de = screen_titles(Locale::German);
    let en = screen_titles(Locale::English);
    assert_eq!(de.len(), en.len(), "both locales list the same screens");

    for ((field, de_title), (_, en_title)) in de.iter().zip(en.iter()) {
        assert_translated(&format!("{field} title"), de_title, en_title);
    }
}

// @scenario: groups-list :: screen renders in the active locale
// @internal
#[test]
fn groups_list_renders_the_active_locale() {
    load_german();
    assert_translated(
        "view-mode toggle label",
        &groups_view_mode_label(Locale::German),
        &groups_view_mode_label(Locale::English),
    );
}

// @scenario: contact-visibility :: the contact name survives interpolation
// @internal
#[test]
fn contact_visibility_title_interpolates_the_contact_name() {
    load_german();
    // Interpolation IS core's to hold, whatever the surrounding wording.
    for locale in [Locale::English, Locale::German] {
        let title = ContactVisibilityEngine::new("Alice".into(), vec![])
            .with_locale(locale)
            .current_screen()
            .title;
        assert!(
            title.contains("Alice"),
            "{locale:?} visibility title dropped the contact name, got {title:?}"
        );
    }
}

// English stays exactly as before (regression pin) — one representative
// check. English is the source language and ships bundled, so pinning it
// couples nothing external.
// @internal
#[test]
fn batch3_english_copy_unchanged() {
    assert_eq!(
        ArchivedContactsEngine::new(vec![]).current_screen().title,
        "Archived Contacts"
    );
    assert_eq!(
        LockScreenEngine::new(5).current_screen().title,
        "Enter Password"
    );
    assert_eq!(
        DuplicateDetectionEngine::new(vec![]).current_screen().title,
        "Duplicate Detection"
    );
    assert_eq!(
        SupportEngine::new().current_screen().title,
        "Support Vauchi"
    );
}

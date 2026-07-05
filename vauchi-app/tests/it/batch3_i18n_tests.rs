// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-14 (`2026-07-05-core-screens-bypass-i18n`): the final 16 M3 S5
//! files render in the user's locale. Keys under each file's own
//! namespace (locales!113). Exact German assertions per CC-03. One
//! test per engine covering the screen title — full a11y/copy detail
//! stays in each engine's own inline unit tests.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ArchivedContactsEngine, ChangePasswordEngine, ContactVisibilityEngine, DecoyContactsEngine,
    DeepLinkConsentEngine, DuplicateDetectionEngine, FingerprintVerifyEngine, GroupInfo,
    GroupsEngine, GroupsMode, HelpEngine, LinkResponderEngine, LockScreenEngine, PlacesEngine,
    SupportEngine, TagPromotionEngine, WorkflowEngine,
};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn deep_link_payload() -> vauchi_core::exchange::link_mode::DeepLinkPayload {
    let (init, _) = initiator_generate();
    parse_exchange_deep_link(&init.url).expect("canonical URL parses")
}

// @scenario: archived-contacts :: screen renders in the active locale
// @internal
#[test]
fn archived_contacts_renders_german() {
    load_german();
    let engine = ArchivedContactsEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Archivierte Kontakte");
}

// @scenario: contact-visibility :: screen renders in the active locale
// @internal
#[test]
fn contact_visibility_renders_german() {
    load_german();
    let engine = ContactVisibilityEngine::new("Alice".into(), vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Sichtbarkeit: Alice");
}

// @scenario: activity-log :: screen renders in the active locale
// @internal
#[test]
fn activity_log_renders_german() {
    load_german();
    let engine = vauchi_app::ui::ActivityLogEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Aktivität");
}

// @scenario: lock-screen :: screen renders in the active locale
// @internal
#[test]
fn lock_screen_renders_german() {
    load_german();
    let engine = LockScreenEngine::new(5).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Passwort eingeben");
}

// @scenario: tag-promotion :: screen renders in the active locale
// @internal
#[test]
fn tag_promotion_renders_german() {
    load_german();
    let engine = TagPromotionEngine::new("t1".into(), "Friends".into(), 3, vec![])
        .with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Tag befördern");
}

// @scenario: duplicate-detection :: screen renders in the active locale
// @internal
#[test]
fn duplicate_detection_renders_german() {
    load_german();
    let engine = DuplicateDetectionEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Duplikaterkennung");
}

// @scenario: decoy-contacts :: screen renders in the active locale
// @internal
#[test]
fn decoy_contacts_renders_german() {
    load_german();
    let engine = DecoyContactsEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Tarnkontakte");
}

// @scenario: deep-link-consent :: screen renders in the active locale
// @internal
#[test]
fn deep_link_consent_renders_german() {
    load_german();
    let engine = DeepLinkConsentEngine::new(deep_link_payload()).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Austauschanfrage");
}

// @scenario: link-responder :: waiting screen renders in the active locale
// @internal
#[test]
fn link_responder_renders_german() {
    load_german();
    let engine = LinkResponderEngine::new(deep_link_payload()).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Warten auf Antwort");
}

// @scenario: groups-list :: screen renders in the active locale
// @internal
#[test]
fn groups_list_renders_german() {
    load_german();
    let groups = vec![GroupInfo {
        id: "g1".into(),
        name: "Work".into(),
        member_count: 1,
        visible_field_count: 1,
    }];
    let engine = GroupsEngine::new(groups, GroupsMode::Members).with_locale(Locale::German);
    let screen = engine.current_screen();
    let has_view_mode = screen.components.iter().any(|c| {
        matches!(c, vauchi_app::ui::Component::ToggleList { label, .. } if label == "Ansichtsmodus")
    });
    assert!(
        has_view_mode,
        "View Mode toggle label must render in German"
    );
}

// @scenario: change-password :: screen renders in the active locale
// @internal
#[test]
fn change_password_renders_german() {
    load_german();
    let engine = ChangePasswordEngine::new(true).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Passwort ändern");
}

// @scenario: fingerprint-verify :: screen renders in the active locale
// @internal
#[test]
fn fingerprint_verify_renders_german() {
    load_german();
    let engine =
        FingerprintVerifyEngine::new("c1", "AA:BB", "CC:DD", false).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Fingerabdruck überprüfen");
}

// @scenario: places-list :: screen renders in the active locale
// @internal
#[test]
fn places_list_renders_german() {
    load_german();
    let engine = PlacesEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Orte");
}

// @scenario: help :: screen renders in the active locale
// @internal
#[test]
fn help_renders_german() {
    load_german();
    let engine = HelpEngine::new(vec![]).with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Hilfe & FAQ");
}

// @scenario: support :: screen renders in the active locale
// @internal
#[test]
fn support_renders_german() {
    load_german();
    let engine = SupportEngine::new().with_locale(Locale::German);
    assert_eq!(engine.current_screen().title, "Vauchi unterstützen");
}

// English stays exactly as before (regression pin) — one representative check.
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

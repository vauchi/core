// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Copy Core wrote as English literals renders in the active locale:
//! the demo tips (vauchi/private#878), the backup reminder toast (#832)
//! and the six help answers outside the highlights (#863). Keys in
//! locales!231.
//!
//! Asserts that the copy resolved a translation, not what it says — see
//! `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, RenderContext, WorkflowEngine,
};
use vauchi_core::Vauchi;
use vauchi_core::types::{BackupReminderState, ReminderFrequency};

fn engine_in(locale: Option<&str>, vauchi: Vauchi) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    engine.set_render_context(RenderContext {
        locale: locale.map(Into::into),
        ..RenderContext::default()
    });
    engine
}

fn vauchi_with_identity() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
}

/// `(text, action label)` of the demo-tip banner on Contacts.
fn demo_banner(locale: Option<&str>) -> (String, String) {
    let vauchi = vauchi_with_identity();
    vauchi.initialize_demo_contact().expect("init demo");
    let mut engine = engine_in(locale, vauchi);
    engine.navigate_to(AppScreen::Contacts);
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Banner {
                text, action_label, ..
            } => Some((text, action_label)),
            _ => None,
        })
        .expect("the demo tip shows on Contacts")
}

// @scenario: demo_contact :: Demo contact appears for users with no contacts
// @internal
#[test]
fn the_demo_tip_and_its_dismiss_action_render_in_the_active_locale() {
    load_german();
    let (de_text, de_action) = demo_banner(Some("de"));
    let (en_text, en_action) = demo_banner(None);

    assert_translated("demo tip", &de_text, &en_text);
    assert_translated("dismiss action", &de_action, &en_action);
}

// @internal
#[test]
fn the_privacy_tip_no_longer_says_data_never_touches_a_server() {
    let content = get_string(Locale::ENGLISH, "demo_tip.privacy.content");

    assert!(
        !content.contains("never touches our servers"),
        "{content:?}"
    );
    assert!(content.contains("cannot read"), "{content:?}");
}

fn reminder_message(locale: Option<&str>) -> String {
    let vauchi = vauchi_with_identity();
    let mut state = BackupReminderState::new();
    state.frequency = ReminderFrequency::Weekly;
    state.last_backup_timestamp = Some(1);
    vauchi.save_backup_reminder_state(&state).unwrap();
    match engine_in(locale, vauchi).drain_backup_reminder() {
        Some(ActionResult::ShowToast { message, .. }) => message,
        other => panic!("a due reminder shows a toast, got {other:?}"),
    }
}

// @scenario: backup_reminder :: First reminder fires 7 days after identity creation
// @internal
#[test]
fn the_backup_reminder_toast_renders_in_the_active_locale() {
    load_german();

    assert_translated(
        "backup reminder",
        &reminder_message(Some("de")),
        &reminder_message(None),
    );
}

const HELP_ANSWERS: [&str; 6] = [
    "add_contact",
    "e2e_encryption",
    "create_backup",
    "social_recovery",
    "exchange_qr",
    "ip_privacy",
];

fn help_screen(locale: Option<&str>) -> String {
    let mut engine = engine_in(locale, vauchi_with_identity());
    engine.navigate_to(AppScreen::Help);
    format!("{:?}", engine.current_screen())
}

// @scenario: privacy_compliance :: Understand E2E encryption
// @internal
#[test]
fn every_help_question_renders_in_the_active_locale() {
    load_german();
    let german = help_screen(Some("de"));

    for faq in HELP_ANSWERS {
        let question_key = format!("faq.{faq}.question");
        let english = get_string(Locale::ENGLISH, &question_key);
        let translated = get_string(Locale::GERMAN, &question_key);
        assert_translated(&question_key, &translated, &english);
        assert!(
            german.contains(&format!("{translated:?}")),
            "{question_key} not on the German Help screen"
        );
        assert!(
            !german.contains(&format!("{english:?}")),
            "{question_key} still English on the German Help screen"
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the multi-stage exchange
//! camera-permission-denied screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{MultiStageExchangeEngine, RepeatEnding, RepeatExchange, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::exchange::ProtocolState;

/// `(title, grant-permission action label)` after camera permission is denied.
fn camera_denied_copy(locale: Locale) -> (String, String) {
    let mut engine = MultiStageExchangeEngine::new_glance().with_locale(locale);
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    let screen = engine.current_screen();
    (
        screen.title.clone(),
        action_label(&screen, "grant_camera_permission"),
    )
}

// @scenario: multi-stage-exchange :: camera-permission-denied screen renders in the active locale
// @internal
#[test]
fn multi_stage_camera_denied_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_grant) = camera_denied_copy(Locale::GERMAN);
    let (en_title, en_grant) = camera_denied_copy(Locale::ENGLISH);

    assert_translated("exchange title", &de_title, &en_title);
    assert_translated("grant-permission action", &de_grant, &en_grant);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn multi_stage_camera_denied_screen_english_copy_unchanged() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Exchange");
    assert_eq!(
        action_label(&screen, "grant_camera_permission"),
        "Grant Permission"
    );
}

const EVERY_REPEAT_STATE: [RepeatExchange; 5] = [
    RepeatExchange::Asking,
    RepeatExchange::Waiting,
    RepeatExchange::Ended(RepeatEnding::KeptByYou),
    RepeatExchange::Ended(RepeatEnding::KeptByThem),
    RepeatExchange::Ended(RepeatEnding::TimedOut),
];

/// The screen of a finalized repeat exchange with Dana, as JSON.
fn repeat_screen(locale: Locale, repeat: RepeatExchange) -> String {
    let mut engine = MultiStageExchangeEngine::new_hover().with_locale(locale);
    engine.set_state(ProtocolState::Finalized);
    engine.set_finalized("Dana".into());
    engine.set_repeat(Some(repeat));
    serde_json::to_string(&engine.current_screen()).unwrap()
}

// @internal
#[test]
fn the_reconnect_question_names_the_peer_and_offers_both_answers() {
    let question = repeat_screen(Locale::ENGLISH, RepeatExchange::Asking);

    for expected in [
        "You already have Dana",
        "Do this only if Dana is the person with you.",
        "Start a new connection",
        "Leave it",
    ] {
        assert!(question.contains(expected), "missing: {expected}");
    }
}

// @internal
#[test]
fn every_repeat_exchange_screen_resolves_its_strings_in_both_languages() {
    load_german();
    for repeat in EVERY_REPEAT_STATE {
        let (en, de) = (
            repeat_screen(Locale::ENGLISH, repeat),
            repeat_screen(Locale::GERMAN, repeat),
        );

        assert!(!en.contains("Missing:"), "{repeat:?}: {en}");
        assert!(!de.contains("Missing:"), "{repeat:?}: {de}");
        assert_ne!(en, de, "{repeat:?} is not translated");
    }
}

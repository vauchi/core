// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine` navigation: `navigate_to`,
//! `navigate_back`, and the bootstrap-only `set_initial_screen`.

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn engine_no_identity() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn navigate_to_pushes_history() {
    let mut engine = engine_with_identity();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);

    engine.navigate_to(AppScreen::Settings);
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "navigate_back should pop MyInfo from history"
    );
}

// @internal
#[test]
fn navigate_back_chain_returns_through_history() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::More);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Privacy);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::More);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
}

// @internal
#[test]
fn navigate_back_with_empty_history_returns_my_info() {
    let mut engine = engine_with_identity();
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "empty history should fall back to MyInfo"
    );
}

// @internal
#[test]
fn set_initial_screen_does_not_push_history() {
    // Without identity, AppEngine::new initializes to Onboarding.
    // A frontend that detects identity at startup needs to swap to
    // MyInfo without leaving Onboarding in the history.
    let mut engine = engine_no_identity();
    assert_eq!(*engine.current_app_screen(), AppScreen::Onboarding);

    engine.set_initial_screen(AppScreen::MyInfo);
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);

    // First user navigation pushes MyInfo to history.
    engine.navigate_to(AppScreen::Settings);

    // navigate_back should land on MyInfo, NOT Onboarding.
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "set_initial_screen must not pollute nav_history"
    );
}

// @internal
#[test]
fn set_initial_screen_overwrites_prior_initial() {
    // Multiple calls to set_initial_screen are idempotent — none push.
    let mut engine = engine_no_identity();
    engine.set_initial_screen(AppScreen::MyInfo);
    engine.set_initial_screen(AppScreen::Lock);
    assert_eq!(*engine.current_app_screen(), AppScreen::Lock);

    engine.navigate_to(AppScreen::Settings);
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Lock,
        "navigate_back lands on the most recent set_initial_screen target"
    );
}

// @internal
#[test]
fn navigate_to_after_set_initial_pushes_initial() {
    // The initial screen IS the legitimate prior screen for the first
    // user navigation — pushing it to history is correct.
    let mut engine = engine_no_identity();
    engine.set_initial_screen(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Privacy);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
}

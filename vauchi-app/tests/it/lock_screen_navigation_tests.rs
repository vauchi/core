// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A locked app must offer no way past the lock but the password.
//!
//! Found on a Pixel 3a: with an app password set, a cold launch showed the
//! lock screen *and* a context bar whose navigation overlay listed every
//! destination. Tapping one navigated through — Settings rendered the
//! display name, and the whole app was reachable without the password.
//!
//! Core caused it, so every shell rendering the bar it is sent inherited
//! it. `contextual_surface_for_screen` passed `sidebar_items(locale)` for
//! every screen alike, `AppScreen::Lock` included, and each item was
//! registered as a routed `UserAction::NavigateToTab`. Nothing in dispatch
//! consulted lock state.
//!
//! Two independent guards, because the composition path and the dispatch
//! path can drift apart — which is how this arose. The affordance must not
//! be offered, *and* the route must refuse.
//!
//! Record: `problems/2026-08-12-android-app-password-bypass`.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

/// Identity plus an app password, which is what makes `bootstrap()` choose
/// `AppScreen::Lock`.
fn locked_engine() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    engine
}

/// The lock surface must not publish a navigation affordance. Asserted on
/// the command batch rather than through a shell, because the bar is
/// Core-supplied and every shell renders what it is handed.
// @scenario: generic_presentation_protocol.feature :: Contextual controls expose four stable roles
#[test]
fn locked_surface_publishes_no_navigation() {
    let mut engine = locked_engine();
    engine.navigate_to(AppScreen::Lock);

    let commands = engine.initial_commands().expect("initial commands");
    let bar = commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar { bar, .. } => Some(bar),
            _ => None,
        })
        .expect("the lock surface still gets a context bar (Back/primary remain valid)");

    assert!(
        bar.navigation.is_none(),
        "a locked app offered a navigation affordance; every destination behind it \
         is reachable without the password",
    );
}

/// Even if a navigation interaction reaches Core — a stale id from an
/// earlier revision, or a future composition path that regresses — the
/// route itself must refuse while locked.
// @scenario: generic_presentation_protocol.feature :: Unknown input fails closed
#[test]
fn locked_engine_refuses_tab_navigation() {
    let mut engine = locked_engine();
    engine.navigate_to(AppScreen::Lock);

    // Compare against where the engine actually was, rather than naming an
    // id: the rendered `ScreenModel.screen_id` ("lock_screen") is not
    // `AppScreen::Lock.screen_id()` ("lock"), and hardcoding either invites
    // a green test that never checked anything.
    let before = engine.current_screen().screen_id;
    let _ = engine.handle_action(UserAction::NavigateToTab {
        action_id: AppScreen::Contacts.screen_id().to_string(),
    });

    assert_eq!(
        engine.current_screen().screen_id,
        before,
        "a NavigateToTab while locked moved the engine off the lock screen",
    );
}

/// The guard must be lock-specific, not a blanket refusal — an unlocked app
/// still navigates. Without this the fix could pass by breaking navigation
/// everywhere.
// @internal
#[test]
fn unlocked_engine_still_navigates_between_tabs() {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);

    let _ = engine.handle_action(UserAction::NavigateToTab {
        action_id: AppScreen::Contacts.screen_id().to_string(),
    });

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Contacts.screen_id(),
        "tab navigation must still work when the app is not locked",
    );
}

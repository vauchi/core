// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Back never leaves the lock screen (vauchi/private#639).
//!
//! Found on an iPhone SE: with the app unlocked on a settings screen, sent
//! to the background and reopened, Core's lock screen offered Back, and one
//! press returned to the settings screen with no password entered. Locking
//! navigated to `AppScreen::Lock` like any other screen, so Back popped the
//! history entry behind it. Every shell that offers Back — iOS's navigation
//! Back, Android's system Back, the TUI's Esc — could leave the lock.
//!
//! `lock_screen_navigation_tests` closed the tab-navigation route
//! (`problems/2026-08-12-android-app-password-bypass`); this closes Back.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{ActionSpec, Command, Event, SurfaceId};

const PASSWORD: &str = "pass1234";
const DURESS_PIN: &str = "654321";

/// Unlocked with `pin`, then sent to the background from Settings → Advanced,
/// so there is history behind the lock screen.
fn locked_after_backgrounding(pin: &str) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(PASSWORD).unwrap();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    engine.vauchi_mut().authenticate(pin).unwrap();
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::SettingsAdvanced);
    let _ = engine.dispatch(Event::AppBackgrounded).unwrap();
    assert_eq!(
        engine.current_screen().screen_id,
        "lock_screen",
        "precondition: locked"
    );
    engine
}

/// The active surface, the last one the batch replaces, and its context bar.
fn active_surface(engine: &mut AppEngine) -> (SurfaceId, Option<ActionSpec>) {
    let commands = engine.initial_commands().unwrap();
    let surface_id = commands
        .iter()
        .rev()
        .find_map(|c| match c {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            _ => None,
        })
        .expect("an active surface");
    let back = commands
        .iter()
        .rev()
        .find_map(|c| match c {
            Command::SetContextBar {
                surface_id: bar_surface,
                bar,
                ..
            } if *bar_surface == surface_id => Some(bar.back.clone()),
            _ => None,
        })
        .expect("a context bar for the active surface");
    (surface_id, back)
}

/// Unlocked, on Settings → Advanced, with Settings behind it.
fn unlocked_on_advanced_settings() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::SettingsAdvanced);
    engine
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn back_while_locked_stays_on_the_lock_screen() {
    for pin in [PASSWORD, DURESS_PIN] {
        let mut engine = locked_after_backgrounding(pin);
        let (surface_id, _) = active_surface(&mut engine);

        let _ = engine.dispatch(Event::BackRequested { surface_id });

        assert_eq!(
            engine.current_screen().screen_id,
            "lock_screen",
            "Back left the lock screen without the password (session unlocked with {pin})",
        );
    }
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn a_direct_navigate_back_while_locked_stays_on_the_lock_screen() {
    let mut engine = locked_after_backgrounding(PASSWORD);

    let _ = engine.handle_action(UserAction::NavigateBack);

    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

/// `navigate_back` is public; a caller that skips `can_go_back` must not
/// pop past the lock either.
// @scenario: security.feature :: App lock with PIN
#[test]
fn navigate_back_called_directly_while_locked_stays_on_the_lock_screen() {
    let mut engine = locked_after_backgrounding(PASSWORD);

    let screen = engine.navigate_back();

    assert_eq!(screen.screen_id, "lock_screen");
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// @scenario: security.feature :: App lock with PIN
#[test]
fn the_locked_surface_offers_no_back() {
    let mut engine = locked_after_backgrounding(PASSWORD);

    let (_, back) = active_surface(&mut engine);

    assert_eq!(back, None, "the lock screen offered Back");
}

// The guard must be lock-specific: every other Back still goes back, by
// each route a shell uses (the bar's Back action, a native Back request,
// and the direct action), and unlocked screens still offer it.

// @internal
#[test]
fn unlocked_screens_still_offer_back() {
    let mut engine = unlocked_on_advanced_settings();

    let (_, back) = active_surface(&mut engine);

    let back = back.expect("Settings → Advanced lost its Back");
    assert!(back.enabled, "Settings → Advanced offers a disabled Back");
}

// @internal
#[test]
fn a_native_back_request_still_goes_back_when_unlocked() {
    let mut engine = unlocked_on_advanced_settings();
    let (surface_id, _) = active_surface(&mut engine);

    let _ = engine
        .dispatch(Event::BackRequested { surface_id })
        .unwrap();

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Settings.screen_id()
    );
}

// @internal
#[test]
fn the_bar_back_action_still_goes_back_when_unlocked() {
    let mut engine = unlocked_on_advanced_settings();
    let (surface_id, back) = active_surface(&mut engine);

    let _ = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: back.expect("Back offered").interaction_id,
        })
        .unwrap();

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Settings.screen_id()
    );
}

// @internal
#[test]
fn a_direct_navigate_back_still_goes_back_when_unlocked() {
    let mut engine = unlocked_on_advanced_settings();

    let _ = engine.handle_action(UserAction::NavigateBack);

    assert_eq!(
        engine.current_screen().screen_id,
        AppScreen::Settings.screen_id()
    );
}

/// After the password, the app is where it was before it locked.
// @scenario: security.feature :: App lock with PIN
#[test]
fn unlocking_after_a_refused_back_still_opens_the_app() {
    let mut engine = locked_after_backgrounding(PASSWORD);
    let (surface_id, _) = active_surface(&mut engine);
    let _ = engine.dispatch(Event::BackRequested { surface_id });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: PASSWORD.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    assert_ne!(engine.current_screen().screen_id, "lock_screen");
}

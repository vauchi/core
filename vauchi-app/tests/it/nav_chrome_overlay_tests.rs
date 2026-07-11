// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the AppEngine nav-chrome overlay (`apply_nav_chrome_overlay`).
//!
//! Core injects global top-bar chrome actions (the Settings gear) into
//! `ScreenModel::nav_actions` on the home screen, so frontends render a
//! generic top-bar affordance instead of a hardcoded native gear —
//! retires android's `ReadyScreen`/`isHomeTab` gate and the iOS `HomeView`
//! header (`2026-07-06-mobile-domain-shell-violations`).

use vauchi_app::ui::{AppEngine, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn engine_no_identity() -> AppEngine {
    AppEngine::new(Vauchi::in_memory().unwrap())
}

// @internal
#[test]
fn home_screen_offers_open_settings_nav_action() {
    let engine = engine_with_identity();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");
    let gear = screen
        .nav_actions
        .iter()
        .find(|a| a.id == "open_settings")
        .expect("home screen must offer the open_settings nav action");
    assert_eq!(gear.label, "Settings");
    assert!(gear.enabled);
}

// @internal
#[test]
fn non_home_screen_has_no_settings_nav_action() {
    // A fresh engine with no identity boots to onboarding, not the home
    // screen — the overlay must leave its `nav_actions` empty.
    let engine = engine_no_identity();
    let screen = engine.current_screen();
    assert_ne!(screen.screen_id, "my_info");
    assert!(
        screen.nav_actions.iter().all(|a| a.id != "open_settings"),
        "only the home screen carries the Settings gear, got {:?}",
        screen.nav_actions
    );
}

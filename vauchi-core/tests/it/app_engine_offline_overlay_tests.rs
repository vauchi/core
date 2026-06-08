// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine offline overlay tests (audit
//! `2026-04-28-lifecycle-session-residue-umbrella` item P2-D).
//!
//! Exercises the `set_network_online` / `apply_offline_overlay` path:
//! when the frontend reports `online == false`, every `ScreenModel`
//! emitted by `current_screen()` and through `ActionResult::*` carries
//! a presentational offline `Component::Banner` injected by core. No
//! frontend `isOnline` mirror flag, no frontend `OfflineBanner()`
//! switch — the renderer just walks the components.

use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::api::Vauchi;

const OFFLINE_ACTION_ID: &str = "offline_banner";

fn unlocked_engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn has_offline_banner(screen: &vauchi_app::ui::ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(
            c,
            Component::Banner { action_id, .. } if action_id == OFFLINE_ACTION_ID
        )
    })
}

// @internal
#[test]
fn online_default_emits_no_offline_banner() {
    let engine = unlocked_engine();
    let screen = engine.current_screen();
    assert!(
        !has_offline_banner(&screen),
        "online (default) state must not inject the offline banner"
    );
}

// @internal
#[test]
fn set_offline_injects_banner_into_current_screen() {
    let mut engine = unlocked_engine();
    engine.set_network_online(false);
    let screen = engine.current_screen();
    assert!(
        has_offline_banner(&screen),
        "offline state must inject the offline banner"
    );
}

// @internal
#[test]
fn flipping_back_online_removes_banner_on_next_render() {
    let mut engine = unlocked_engine();
    engine.set_network_online(false);
    assert!(has_offline_banner(&engine.current_screen()));

    engine.set_network_online(true);
    assert!(
        !has_offline_banner(&engine.current_screen()),
        "going back online must drop the banner from subsequent renders"
    );
}

// @internal
#[test]
fn offline_banner_is_idempotent_across_repeated_renders() {
    let mut engine = unlocked_engine();
    engine.set_network_online(false);

    let banners_per_render = (0..3)
        .map(|_| {
            engine
                .current_screen()
                .components
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        Component::Banner { action_id, .. } if action_id == OFFLINE_ACTION_ID
                    )
                })
                .count()
        })
        .collect::<Vec<_>>();

    for (i, count) in banners_per_render.iter().enumerate() {
        assert_eq!(
            *count, 1,
            "render #{i}: expected exactly one offline banner, got {count}"
        );
    }
}

// @internal
#[test]
fn offline_banner_persists_after_screen_navigation() {
    let mut engine = unlocked_engine();
    engine.set_network_online(false);
    // overlay-applying entrypoint. The banner state is owned by
    // AppEngine, not by the per-screen workflow engine, so it
    // must survive a screen change.
    let _ = engine.navigate_to(AppScreen::Settings);
    let screen = engine.current_screen();
    assert!(
        has_offline_banner(&screen),
        "offline banner state must persist across screen navigation"
    );
}

// @internal
#[test]
fn is_network_online_reflects_setter_state() {
    let mut engine = unlocked_engine();
    assert!(engine.is_network_online(), "default state is online");

    engine.set_network_online(false);
    assert!(!engine.is_network_online());

    engine.set_network_online(true);
    assert!(engine.is_network_online());
}

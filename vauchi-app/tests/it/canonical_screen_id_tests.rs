// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tier-0 (c) narrow collapse: `AppEngine` stamps the canonical
//! `AppScreen::screen_id()` onto outgoing `ScreenModel`s for the
//! multi-state tab families whose engines emit non-canonical sub-state
//! ids (`contact_list`, `groups_list`, `backup_choose`, `duress_overview`,
//! `sync_status`). Frontends then receive a stable id and can retire the
//! `CoreScreenIdMap` fold. Screens *outside* the allow-list keep their
//! engine sub-state id (the collapse is narrow, not blanket) — pinned by
//! the control test so a future widening is caught.

use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The five collapsed families: navigating to each yields a
/// `current_screen().screen_id` equal to the canonical
/// `AppScreen::screen_id()`, not the engine's sub-state id.
// @internal
#[test]
fn collapsed_families_report_canonical_screen_id() {
    let cases = [
        (AppScreen::Contacts, "contacts"),
        (AppScreen::Groups, "groups"),
        (AppScreen::Backup, "backup"),
        (AppScreen::DuressPin, "duress_pin"),
        (AppScreen::Sync, "sync"),
    ];
    for (screen, canonical) in cases {
        let mut engine = engine_with_identity();
        engine.navigate_to(screen.clone());
        assert_eq!(
            engine.current_screen().screen_id,
            canonical,
            "navigating to {screen:?} must report canonical screen_id `{canonical}`"
        );
    }
}

/// Control: a multi-state screen *outside* the allow-list (Exchange,
/// whose engine emits `exchange_mode_selection`) keeps its engine
/// sub-state id. Guards against the collapse silently widening to blanket.
// @internal
#[test]
fn non_allowlisted_screen_keeps_engine_screen_id() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_mode_selection",
        "Exchange is not in the narrow collapse set — its engine sub-state id must survive"
    );
}

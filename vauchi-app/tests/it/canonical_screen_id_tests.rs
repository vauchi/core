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

/// The collapsed families: navigating to each yields a
/// `current_screen().screen_id` equal to the canonical
/// `AppScreen::screen_id()`, not the engine's sub-state id.
///
/// `Exchange` is the special member: its engine emits *many* sub-state
/// ids (`exchange_mode_selection`, `exchange_verifying`,
/// `exchange_success`, `exchange_nfc_role`, …), but only the
/// mode-selection **root** is stamped with the canonical `exchange` id.
/// That root is a bottom-tab root, and Android's tab-bar shows only when
/// `screen_id == tab_id`; without the stamp the Exchange tab rendered no
/// nav bar and system-BACK exited the app
/// (`2026-05-21-android-back-stack-and-bottom-nav-broken`). The sub-state
/// ids are preserved (see `non_allowlisted_screen_keeps_engine_screen_id`).
// @internal
#[test]
fn collapsed_families_report_canonical_screen_id() {
    let cases = [
        (AppScreen::Contacts, "contacts"),
        (AppScreen::Groups, "groups"),
        (AppScreen::Backup, "backup"),
        (AppScreen::DuressPin, "duress_pin"),
        (AppScreen::Sync, "sync"),
        // Mode-selection root only — sub-states keep their engine ids.
        (AppScreen::Exchange, "exchange"),
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

/// Control: a multi-state screen *outside* the allow-list (Recovery,
/// whose engine emits the sub-state id `recovery_status`) keeps its
/// engine sub-state id. Guards against the collapse silently widening to
/// blanket — the narrow set is `Contacts | Groups | Backup | DuressPin |
/// Sync` plus the single `Exchange` mode-selection root.
// @internal
#[test]
fn non_allowlisted_screen_keeps_engine_screen_id() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Recovery);
    assert_eq!(
        engine.current_screen().screen_id,
        "recovery_status",
        "Recovery is not in the narrow collapse set — its engine sub-state id must survive"
    );
}

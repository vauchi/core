// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-3 (`2026-07-03-core-screens-bypass-i18n`): the sync-status
//! screen (connection state, relay details, sync-now/test-connection
//! actions) renders in the user's locale. Keys in `sync.*`
//! (locales!92). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{Component, SyncStatusEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn status_title(engine: &SyncStatusEngine) -> String {
    match &engine.current_screen().components[0] {
        Component::StatusIndicator { title, .. } => title.clone(),
        other => panic!("expected StatusIndicator, got {other:?}"),
    }
}

// @scenario: sync :: sync-status screen renders in the active locale
// @internal
#[test]
fn sync_status_renders_german() {
    load_german();
    let engine =
        SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 0).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Synchronisation");
    assert_eq!(status_title(&engine), "Relay: Offline");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "test_connection")
            .unwrap()
            .label,
        "Verbindung erneut versuchen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn sync_status_english_copy_unchanged() {
    let engine = SyncStatusEngine::new("https://relay.vauchi.app".into(), 5, 0);
    assert_eq!(status_title(&engine), "Relay: Offline");
    assert_eq!(
        engine
            .current_screen()
            .actions
            .iter()
            .find(|a| a.id == "test_connection")
            .unwrap()
            .label,
        "Retry Connection"
    );
}

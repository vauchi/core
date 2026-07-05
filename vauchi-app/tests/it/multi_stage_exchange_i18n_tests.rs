// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-10 (`2026-07-03-core-screens-bypass-i18n`): the multi-stage
//! face-to-face exchange engine renders in the user's locale. Keys in
//! `multi_stage.*` (locales!107). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{MultiStageExchangeEngine, WorkflowEngine};
use vauchi_core::Event;

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: multi-stage-exchange :: camera-permission-denied screen renders in the active locale
// @internal
#[test]
fn multi_stage_camera_denied_screen_renders_german() {
    load_german();
    let mut engine = MultiStageExchangeEngine::new_glance().with_locale(Locale::German);
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Austausch");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "grant_camera_permission")
            .unwrap()
            .label,
        "Berechtigung erteilen"
    );
}

// English stays exactly as before (regression pin).
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
        screen
            .actions
            .iter()
            .find(|a| a.id == "grant_camera_permission")
            .unwrap()
            .label,
        "Grant Permission"
    );
}

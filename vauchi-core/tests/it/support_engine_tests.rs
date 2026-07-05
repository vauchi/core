// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn support_screen_id() {
    let engine = SupportEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "support");
}

// @internal
#[test]
fn support_title() {
    let engine = SupportEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Support Vauchi");
}

// @internal
#[test]
fn support_open_github_sponsors_opens_url() {
    let mut engine = SupportEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_github_sponsors".into(),
    });
    match result {
        ActionResult::OpenUrl { url } => {
            assert_eq!(url, "https://github.com/sponsors/vauchi");
        }
        other => panic!("Expected OpenUrl, got {other:?}"),
    }
}

// @internal
#[test]
fn support_open_liberapay_opens_url() {
    let mut engine = SupportEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_liberapay".into(),
    });
    match result {
        ActionResult::OpenUrl { url } => {
            assert_eq!(url, "https://liberapay.com/vauchi");
        }
        other => panic!("Expected OpenUrl, got {other:?}"),
    }
}

// @internal
#[test]
fn support_unknown_action_returns_update_screen() {
    let mut engine = SupportEngine::new();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "support");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn support_default_creates_engine() {
    let engine = SupportEngine::default();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "support");
    assert_eq!(screen.title, "Support Vauchi");
}

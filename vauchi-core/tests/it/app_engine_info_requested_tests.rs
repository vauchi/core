// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine tests for `UserAction::InfoRequested` interception.

use vauchi_app::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

// @internal
#[test]
fn info_requested_known_key_returns_show_info_overlay() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = engine.handle_action(UserAction::InfoRequested {
        key: "auto_lock".into(),
    });

    match result {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert!(!title.is_empty(), "title should not be empty");
            assert!(
                !title.starts_with("Missing:"),
                "title should not be a missing-key placeholder"
            );
            assert!(!body.is_empty(), "body should not be empty");
            assert!(
                !body.starts_with("Missing:"),
                "body should not be a missing-key placeholder"
            );
        }
        other => panic!("Expected ShowInfoOverlay, got {other:?}"),
    }
}

// @internal
#[test]
fn info_requested_unknown_key_returns_update_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = engine.handle_action(UserAction::InfoRequested {
        key: "nonexistent_key_xyz".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert!(
                !screen.screen_id.is_empty(),
                "screen_id should not be empty on fallback"
            );
        }
        other => panic!("Expected UpdateScreen for unknown key, got {other:?}"),
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::ui::*;

#[test]
fn gdpr_screen_id() {
    let engine = GdprEngine::new(None, "All consents granted".into());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "privacy_settings");
}

#[test]
fn gdpr_title() {
    let engine = GdprEngine::new(None, "All consents granted".into());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Privacy & Data");
}

#[test]
fn gdpr_shows_deletion_status() {
    let engine = GdprEngine::new(Some("Pending".into()), "All consents granted".into());
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "privacy_info", "Deletion Status");
    assert_eq!(detail, "Pending");
}

#[test]
fn gdpr_shows_no_deletion_requested_when_none() {
    let engine = GdprEngine::new(None, "All consents granted".into());
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "privacy_info", "Deletion Status");
    assert_eq!(detail, "No deletion requested");
}

#[test]
fn gdpr_export_completes() {
    let mut engine = GdprEngine::new(None, "All consents granted".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn gdpr_export_collected_input() {
    let mut engine = GdprEngine::new(None, "All consents granted".into());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    assert_eq!(engine.collected_input(), Some("export".into()));
}

#[test]
fn gdpr_delete_completes() {
    let mut engine = GdprEngine::new(None, "All consents granted".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    assert_eq!(result, ActionResult::Complete);
}

#[test]
fn gdpr_delete_collected_input() {
    let mut engine = GdprEngine::new(None, "All consents granted".into());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    assert_eq!(engine.collected_input(), Some("delete".into()));
}

#[test]
fn gdpr_collected_input_initially_none() {
    let engine = GdprEngine::new(None, "All consents granted".into());
    assert_eq!(engine.collected_input(), None);
}

#[test]
fn gdpr_unknown_action_returns_update_screen() {
    let mut engine = GdprEngine::new(None, "All consents granted".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "privacy_settings");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// --- helpers ---

fn find_info_detail(screen: &ScreenModel, panel_id: &str, item_title: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => items
                .iter()
                .find(|item| item.title == item_title)
                .map(|item| item.detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("InfoItem '{item_title}' not found in panel '{panel_id}'"))
}

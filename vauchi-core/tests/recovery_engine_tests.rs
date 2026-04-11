// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn make_contact(id: &str, name: &str, initials: &str) -> ContactItem {
    ContactItem {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        avatar_initials: initials.into(),
        status: None,
        searchable_fields: vec![],
        a11y: None,
    }
}

fn quorum_not_met() -> RecoveryEngine {
    // 1 contact, threshold 3 => quorum not met
    RecoveryEngine::new(vec![make_contact("c1", "Alice", "AL")], 3)
}

fn quorum_met() -> RecoveryEngine {
    // 3 contacts, threshold 3 => quorum met
    RecoveryEngine::new(
        vec![
            make_contact("c1", "Alice", "AL"),
            make_contact("c2", "Bob", "BO"),
            make_contact("c3", "Carol", "CA"),
        ],
        3,
    )
}

#[test]
fn recovery_screen_id() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "recovery_status");
}

#[test]
fn recovery_title() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Social Recovery");
}

#[test]
fn recovery_quorum_not_met_disables_claim() {
    let engine = quorum_not_met();
    let screen = engine.current_screen();

    let claim_action = screen
        .actions
        .iter()
        .find(|a| a.id == "claim")
        .expect("claim action should exist");
    assert!(
        !claim_action.enabled,
        "claim should be disabled when quorum not met"
    );

    // Verify quorum info shows "1 of 3"
    let detail = find_info_detail(&screen, "quorum_info", "Trusted Contacts");
    assert_eq!(detail, "1 of 3");

    let quorum_met_detail = find_info_detail(&screen, "quorum_info", "Quorum Met");
    assert_eq!(quorum_met_detail, "No");
}

#[test]
fn recovery_quorum_met_enables_claim() {
    let engine = quorum_met();
    let screen = engine.current_screen();

    let claim_action = screen
        .actions
        .iter()
        .find(|a| a.id == "claim")
        .expect("claim action should exist");
    assert!(
        claim_action.enabled,
        "claim should be enabled when quorum met"
    );

    // Verify quorum info shows "3 of 3"
    let detail = find_info_detail(&screen, "quorum_info", "Trusted Contacts");
    assert_eq!(detail, "3 of 3");

    let quorum_met_detail = find_info_detail(&screen, "quorum_info", "Quorum Met");
    assert_eq!(quorum_met_detail, "Yes");
}

#[test]
fn recovery_claim_shows_alert() {
    let mut engine = quorum_met();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "claim".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Coming Soon");
            assert_eq!(
                message,
                "Social recovery will be available in a future update."
            );
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn recovery_status_shows_alert() {
    let mut engine = quorum_not_met();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "status".into(),
    });
    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "Recovery Status");
            assert_eq!(message, "No active recovery claims.");
        }
        other => panic!("Expected ShowAlert, got {other:?}"),
    }
}

#[test]
fn recovery_unknown_action_returns_update_screen() {
    let mut engine = quorum_not_met();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "recovery_status");
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

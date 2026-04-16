// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn link_starts_at_show_qr() {
    let engine = DeviceLinkingEngine::new("vauchi://link?token=abc123".to_string());
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "link_show_qr");
    assert_eq!(
        screen.progress,
        Some(Progress {
            current_step: 1,
            total_steps: 4,
            label: None,
        })
    );
}

// @internal
#[test]
fn link_shows_qr_data() {
    let engine = DeviceLinkingEngine::new("vauchi://link?token=abc123".to_string());
    let screen = engine.current_screen();

    assert_eq!(screen.components.len(), 2);
    match &screen.components[0] {
        Component::QrCode {
            id,
            data,
            mode,
            label,
            ..
        } => {
            assert_eq!(id, "qr");
            assert_eq!(data, "vauchi://link?token=abc123");
            assert_eq!(mode, &QrMode::Display);
            assert_eq!(label.as_deref(), Some("Scan on new device"));
        }
        other => panic!("expected QrCode, got {:?}", other),
    }
}

// @internal
#[test]
fn link_peer_connected_shows_verify() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "link_verify");

    match &screen.components[0] {
        Component::Text {
            id, content, style, ..
        } => {
            assert_eq!(id, "code");
            assert_eq!(content, "ABC-123");
            assert_eq!(style, &TextStyle::Title);
        }
        other => panic!("expected Text, got {:?}", other),
    }

    match &screen.components[1] {
        Component::InfoPanel {
            id, title, items, ..
        } => {
            assert_eq!(id, "verify_info");
            assert_eq!(title, "Verify this code");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "Compare codes");
        }
        other => panic!("expected InfoPanel, got {:?}", other),
    }
}

// @internal
#[test]
fn link_confirm_starts_sync() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "link_syncing");
            match &screen.components[0] {
                Component::StatusIndicator { title, status, .. } => {
                    assert_eq!(title, "Syncing data...");
                    assert_eq!(*status, Status::InProgress);
                }
                other => panic!("expected StatusIndicator, got {:?}", other),
            }
            assert!(screen.actions.is_empty());
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn link_reject_restarts() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "reject".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "link_show_qr");
        }
        other => panic!("expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn link_sync_complete_shows_success() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".to_string(),
    });
    engine.sync_complete();

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "link_complete");
    match &screen.components[0] {
        Component::StatusIndicator { title, status, .. } => {
            assert_eq!(title, "Device Linked");
            assert_eq!(*status, Status::Success);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

// @internal
#[test]
fn link_done_completes() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".to_string(),
    });
    engine.sync_complete();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".to_string(),
    });

    assert!(
        matches!(result, ActionResult::Complete),
        "expected Complete, got {:?}",
        result
    );
}

// @internal
#[test]
fn peer_connected_is_noop_outside_show_qr() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    engine.peer_connected("ABC-123".to_string());
    // Now in VerifyCode — calling peer_connected again should be a no-op
    engine.peer_connected("XYZ-999".to_string());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "link_verify");
    // Original code is preserved, not overwritten
    match &screen.components[0] {
        Component::Text { content, .. } => assert_eq!(content, "ABC-123"),
        other => panic!("expected Text, got {:?}", other),
    }
}

// @internal
#[test]
fn sync_complete_is_noop_outside_syncing() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());
    // In ShowQr — sync_complete should be a no-op
    engine.sync_complete();
    assert_eq!(engine.current_screen().screen_id, "link_show_qr");

    // In VerifyCode — sync_complete should be a no-op
    engine.peer_connected("ABC-123".to_string());
    engine.sync_complete();
    assert_eq!(engine.current_screen().screen_id, "link_verify");
}

// @internal
#[test]
fn link_cancel_from_show_qr() {
    let mut engine = DeviceLinkingEngine::new("data".to_string());

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".to_string(),
    });

    assert!(
        matches!(result, ActionResult::Complete),
        "expected Complete, got {:?}",
        result
    );
}

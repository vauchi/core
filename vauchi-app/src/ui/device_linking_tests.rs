// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `device_linking.rs` — extracted to keep the engine
//! file under the 1000-line src hard limit. Loaded via `#[path]`.

use super::*;

// @internal
#[test]
fn new_starts_at_show_qr_for_backwards_compat() {
    let e = DeviceLinkingEngine::new("qr-data".into());
    assert_eq!(e.current_screen().screen_id, "link_show_qr");
}

// @internal
#[test]
fn with_transport_selection_starts_at_transport_picker() {
    let e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_transport");
    assert_eq!(screen.actions.len(), 3); // internet + offline + cancel
    assert_eq!(screen.actions[0].id, TRANSPORT_INTERNET_ACTION_ID);
    assert_eq!(screen.actions[1].id, TRANSPORT_OFFLINE_ACTION_ID);
}

// @internal
#[test]
fn select_internet_advances_to_show_qr() {
    let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: TRANSPORT_INTERNET_ACTION_ID.into(),
    });
    match result {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_show_qr"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn select_offline_advances_to_offline_stub() {
    let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: TRANSPORT_OFFLINE_ACTION_ID.into(),
    });
    match result {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_offline_stub"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn back_from_offline_returns_to_transport() {
    let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let _ = e.handle_action(UserAction::ActionPressed {
        action_id: TRANSPORT_OFFLINE_ACTION_ID.into(),
    });
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: "back_to_transport".into(),
    });
    match result {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_transport"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn cancel_from_transport_emits_complete() {
    let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
}

// @internal
#[test]
fn peer_connected_advances_show_qr_to_verify_code() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.peer_connected("123456".into());
    assert_eq!(e.current_screen().screen_id, "link_verify");
    // Verification code rendered as Text content
    if let Component::Text { content, .. } = &e.current_screen().components[0] {
        assert_eq!(content, "123456");
    } else {
        panic!("expected Text component");
    }
}

// @internal
#[test]
fn confirm_from_verify_advances_to_syncing() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.peer_connected("123456".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });
    match result {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_syncing"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn reject_from_verify_returns_to_show_qr() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.peer_connected("123456".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: "reject".into(),
    });
    match result {
        ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_show_qr"),
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn sync_complete_advances_to_complete() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.peer_connected("123456".into());
    let _ = e.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });
    e.sync_complete();
    assert_eq!(e.current_screen().screen_id, "link_complete");
}

// @internal
#[test]
fn progress_hidden_on_pre_flow_steps() {
    let e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    assert!(e.current_screen().progress.is_none());
}

// ---- Pair 5 receiver-side state coverage ----

// @internal
#[test]
fn transition_to_qr_pending_sets_pending_screen() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_qr_pending();
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_qr_pending");
    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, CANCEL_ACTION_ID);
}

// @internal
#[test]
fn transition_to_waiting_renders_qr_with_expiry() {
    let mut e = DeviceLinkingEngine::new("old".into());
    e.transition_to_waiting_for_request("new-qr-data".into(), 1_700_000_500);
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_waiting");
    let qr = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::QrCode { .. }))
        .expect("QR component present");
    if let Component::QrCode { data, .. } = qr {
        assert_eq!(data, "new-qr-data");
    }
    let expiry = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Text { id, content, .. } if id == "expires_at" => Some(content.clone()),
            _ => None,
        })
        .expect("expires_at text present");
    assert!(expiry.contains("1700000500"));
}

// @internal
#[test]
fn transition_to_qr_expired_shows_retry_and_cancel() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_qr_expired();
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_qr_expired");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
}

// @internal
#[test]
fn confirming_device_screen_shows_name_and_code() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_confirming_device");
    assert_eq!(screen.subtitle.as_deref(), Some("Device: New iPad"));
    let code = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("code text present");
    assert_eq!(code, "654321");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![CODES_MATCH_ACTION_ID, DENY_ACTION_ID]);
}

// @internal
#[test]
fn codes_match_advances_to_verifying_proximity_preserving_code() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: CODES_MATCH_ACTION_ID.into(),
    });
    match result {
        ActionResult::NavigateTo(s) => {
            assert_eq!(s.screen_id, "link_verifying_proximity");
            let code = s
                .components
                .iter()
                .find_map(|c| match c {
                    Component::Text { content, .. } => Some(content.clone()),
                    _ => None,
                })
                .expect("code text present");
            assert_eq!(code, "654321");
        }
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @internal
#[test]
fn deny_from_confirming_device_emits_device_link_deny() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: DENY_ACTION_ID.into(),
    });
    assert!(
        matches!(result, ActionResult::DeviceLinkDeny),
        "expected DeviceLinkDeny, got {result:?}"
    );
}

// @internal
#[test]
fn confirm_manual_emits_typed_result_with_code_and_advances_step() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
    let _ = e.handle_action(UserAction::ActionPressed {
        action_id: CODES_MATCH_ACTION_ID.into(),
    });
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: CONFIRM_MANUAL_ACTION_ID.into(),
    });
    match result {
        ActionResult::DeviceLinkConfirmManual { code } => assert_eq!(code, "654321"),
        other => panic!("expected DeviceLinkConfirmManual, got {other:?}"),
    }
    // Step still advanced — next render shows the Completing screen.
    assert_eq!(e.current_screen().screen_id, "link_completing");
}

// @internal
#[test]
fn transition_to_completing_uses_completing_screen() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
    e.transition_to_completing();
    assert_eq!(e.current_screen().screen_id, "link_completing");
}

// @internal
#[test]
fn transition_to_link_success_uses_complete_screen() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_link_success();
    assert_eq!(e.current_screen().screen_id, "link_complete");
}

// @internal
#[test]
fn transition_to_link_failed_renders_message() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_link_failed("relay unreachable".into());
    let screen = e.current_screen();
    assert_eq!(screen.screen_id, "link_failed");
    let detail = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        })
        .expect("status detail present");
    assert_eq!(detail, "relay unreachable");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
}

// @internal
#[test]
fn retry_from_qr_expired_emits_device_link_retry_and_advances_step() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_qr_expired();
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: RETRY_ACTION_ID.into(),
    });
    assert!(
        matches!(result, ActionResult::DeviceLinkRetry),
        "expected DeviceLinkRetry, got {result:?}"
    );
    assert_eq!(e.current_screen().screen_id, "link_qr_pending");
}

// @internal
#[test]
fn retry_from_link_failed_emits_device_link_retry_and_advances_step() {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_link_failed("oops".into());
    let result = e.handle_action(UserAction::ActionPressed {
        action_id: RETRY_ACTION_ID.into(),
    });
    assert!(
        matches!(result, ActionResult::DeviceLinkRetry),
        "expected DeviceLinkRetry, got {result:?}"
    );
    assert_eq!(e.current_screen().screen_id, "link_qr_pending");
}

// @internal
#[test]
fn cancel_is_terminal_from_every_new_state() {
    for setup in [
        |e: &mut DeviceLinkingEngine| e.transition_to_qr_pending(),
        |e: &mut DeviceLinkingEngine| e.transition_to_waiting_for_request("qr".into(), 1),
        |e: &mut DeviceLinkingEngine| e.transition_to_qr_expired(),
        |e: &mut DeviceLinkingEngine| {
            e.transition_to_confirming_device("D".into(), "1".into(), "ab".into())
        },
        |e: &mut DeviceLinkingEngine| e.transition_to_completing(),
        |e: &mut DeviceLinkingEngine| e.transition_to_link_failed("x".into()),
    ] {
        let mut e = DeviceLinkingEngine::new("qr".into());
        setup(&mut e);
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: CANCEL_ACTION_ID.into(),
        });
        assert!(
            matches!(result, ActionResult::Complete),
            "expected Complete, got {result:?}"
        );
    }
}

// @internal
#[test]
fn progress_hidden_on_qr_expired_and_link_failed() {
    let mut expired = DeviceLinkingEngine::new("qr".into());
    expired.transition_to_qr_expired();
    assert!(expired.current_screen().progress.is_none());

    let mut failed = DeviceLinkingEngine::new("qr".into());
    failed.transition_to_link_failed("x".into());
    assert!(failed.current_screen().progress.is_none());
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the receiver-side `DeviceLinkingEngine`
//! bridge methods on `AppEngine`. Pair 5 of
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`.
//!
//! Each test navigates to the device-linking screen, drives the
//! corresponding cycle-thread bridge method, and asserts the engine
//! state is reflected in the rendered `ScreenModel`. Bridges return
//! `None` from any other screen; the off-screen guards are covered
//! at the bottom of the file.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_on_device_linking() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::DeviceLinking);
    engine
}

// @scenario: pair5_device_link_bridge :: cycle thread can push QrPending into the engine
#[test]
fn qr_pending_bridge_renders_pending_screen() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_qr_pending()
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_qr_pending");
}

// @scenario: pair5_device_link_bridge :: on_qr_ready surfaces QR data + expiry
#[test]
fn qr_ready_bridge_renders_waiting_with_qr() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_qr_ready("fresh-qr".into(), 1_700_000_500)
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_waiting");
    let qr = screen
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::QrCode { data, .. } => Some(data.clone()),
            _ => None,
        })
        .expect("QR component present");
    assert_eq!(qr, "fresh-qr");
}

// @scenario: pair5_device_link_bridge :: qr_expired failure renders retry+cancel surface
#[test]
fn qr_expired_bridge_renders_expired_screen() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_qr_expired()
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_qr_expired");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["retry", "cancel"]);
}

// @scenario: pair5_device_link_bridge :: on_confirmation_required surfaces device name + code
#[test]
fn request_received_bridge_renders_confirming_device_screen() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_request_received("New iPad".into(), "112233".into(), "abcd".into())
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_confirming_device");
    assert_eq!(screen.subtitle.as_deref(), Some("Device: New iPad"));
}

// @scenario: pair5_device_link_bridge :: codes_match preserves confirmation code into proximity step
#[test]
fn request_received_then_codes_match_advances_to_proximity_with_code() {
    let mut engine = engine_on_device_linking();
    let _ = engine.device_link_request_received("New iPad".into(), "112233".into(), "abcd".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "codes_match".into(),
    });
    match result {
        ActionResult::NavigateTo(s) => {
            assert_eq!(s.screen_id, "link_verifying_proximity");
            let code = s
                .components
                .iter()
                .find_map(|c| match c {
                    vauchi_app::ui::Component::Text { content, .. } => Some(content.clone()),
                    _ => None,
                })
                .expect("code text present");
            assert_eq!(code, "112233");
        }
        other => panic!("expected NavigateTo, got {other:?}"),
    }
}

// @scenario: pair5_device_link_bridge :: confirm_manual emits typed action result with code
#[test]
fn confirm_manual_emits_typed_result_carrying_code() {
    let mut engine = engine_on_device_linking();
    let _ = engine.device_link_request_received("New iPad".into(), "112233".into(), "abcd".into());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "codes_match".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_manual".into(),
    });
    match result {
        ActionResult::DeviceLinkConfirmManual { code } => assert_eq!(code, "112233"),
        other => panic!("expected DeviceLinkConfirmManual, got {other:?}"),
    }
    assert_eq!(engine.current_screen().screen_id, "link_completing");
}

// @scenario: pair5_device_link_bridge :: deny on confirming device emits DeviceLinkDeny
#[test]
fn deny_emits_device_link_deny_result() {
    let mut engine = engine_on_device_linking();
    let _ = engine.device_link_request_received("New iPad".into(), "112233".into(), "abcd".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "deny".into(),
    });
    assert!(
        matches!(result, ActionResult::DeviceLinkDeny),
        "expected DeviceLinkDeny, got {result:?}"
    );
}

// @scenario: pair5_device_link_bridge :: retry from expired emits DeviceLinkRetry and advances to pending
#[test]
fn retry_from_expired_emits_device_link_retry_and_advances_to_pending() {
    let mut engine = engine_on_device_linking();
    let _ = engine.device_link_qr_expired();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".into(),
    });
    assert!(
        matches!(result, ActionResult::DeviceLinkRetry),
        "expected DeviceLinkRetry, got {result:?}"
    );
    assert_eq!(engine.current_screen().screen_id, "link_qr_pending");
}

// @scenario: pair5_device_link_bridge :: on_completed transitions to terminal Complete screen
#[test]
fn completed_bridge_renders_complete_screen() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_completed()
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_complete");
}

// @scenario: pair5_device_link_bridge :: on_failed maps to honest copy in the LinkFailed screen
// M5 B2: the raw machine failure id never reaches the screen; the
// stable reason is mapped to a user-facing sentence
// (2026-07-03-second-device-join-dead-end item 4).
#[test]
fn failed_bridge_renders_honest_failure_copy() {
    let mut engine = engine_on_device_linking();
    let screen = engine
        .device_link_failed("user_confirm_timeout".into())
        .expect("on device linking screen");
    assert_eq!(screen.screen_id, "link_failed");
    let detail = screen
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        })
        .expect("status detail present");
    assert!(
        !detail.contains("user_confirm_timeout"),
        "raw failure id must never render: {detail}"
    );
    assert!(
        detail.contains("60 seconds"),
        "confirm-timeout copy surfaces the 60s window: {detail}"
    );
}

// @scenario: pair5_device_link_bridge :: cancel from any device-link step navigates away
#[test]
fn cancel_from_any_step_navigates_away_from_device_linking() {
    // The engine emits ActionResult::Complete on cancel; AppEngine's
    // top-level routing then runs `handle_completion()` which leaves
    // the device-linking screen. PlatformAppEngine's session-aware
    // wiring will additionally call `MobileDeviceLinkSession::cancel`
    // before this navigation lands.
    let mut engine = engine_on_device_linking();
    let _ = engine.device_link_request_received("New iPad".into(), "112233".into(), "abcd".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    let screen = match result {
        ActionResult::NavigateTo(s) => s,
        other => panic!("expected NavigateTo away from device_linking, got {other:?}"),
    };
    assert_ne!(
        screen.screen_id, "link_confirming_device",
        "still on device-link confirm step after cancel"
    );
    assert_ne!(
        screen.screen_id, "link_qr_pending",
        "still on device-link pending step after cancel"
    );
}

// ── Off-screen guards ──────────────────────────────────────────

// @scenario: pair5_device_link_bridge :: bridges no-op (return None) off the device-linking screen
#[test]
fn bridges_return_none_when_not_on_device_linking_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::Settings);

    assert!(engine.device_link_qr_pending().is_none());
    assert!(engine.device_link_qr_ready("q".into(), 0).is_none());
    assert!(engine.device_link_qr_expired().is_none());
    assert!(
        engine
            .device_link_request_received("d".into(), "c".into(), "ab".into())
            .is_none()
    );
    assert!(engine.device_link_completed().is_none());
    assert!(engine.device_link_failed("reason".into()).is_none());
}

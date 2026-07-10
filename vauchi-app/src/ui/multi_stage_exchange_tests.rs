// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `multi_stage_exchange.rs` — extracted to keep the
//! engine file under the src size limit. Loaded via `#[path]`; stays a
//! unit-test child module (private-field access preserved).

use super::*;

fn engine_with_state(state: ProtocolState) -> MultiStageExchangeEngine {
    let mut e = MultiStageExchangeEngine::new_glance();
    e.set_state(state);
    e
}

// @internal
#[test]
fn current_screen_stamps_native_wrapper_hint() {
    // The multi-stage engine renders inside the native hardware wrapper
    // on mobile; core owns the decision via `native_wrapper_hint`
    // (`2026-07-06-mobile-domain-shell-violations` I5/A2).
    let engine = engine_with_state(ProtocolState::Idle);
    let screen = engine.current_screen();
    assert_eq!(
        screen.native_wrapper_hint,
        NativeWrapperHint::MultiStageExchange
    );
}

// @internal
#[test]
fn retry_routes_to_mode_picker_via_cancelled_complete() {
    // Retry on the Failed screen returns the user to the exchange
    // mode-selection picker (not an in-place restart). It returns
    // `Complete` with `cancelled` set, which `handle_completion`
    // (routing.rs:470-486) routes to `AppScreen::Exchange` rather
    // than Contacts.
    let mut engine = engine_with_state(ProtocolState::Failed("boom".into()));
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: RETRY_ACTION_ID.into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "Retry must return Complete so the AppEngine routes the navigation, got {result:?}",
    );
    assert!(
        engine.was_cancelled(),
        "Retry must set cancelled so completion lands on the mode picker, not Contacts",
    );
}

// @internal
#[test]
fn success_screen_renders_rich_summary_when_attached() {
    // Finalized + session_ended routes build_screen to the success
    // screen; with a summary attached it renders the rich, shared
    // core-driven chrome (2026-06-04-exchange-terminal-screens).
    let mut engine = engine_with_state(ProtocolState::Finalized);
    engine.session_ended = true;
    engine.set_success_summary(crate::ui::exchange::success::ExchangeSuccessSummary {
        peer_name: "Bob".into(),
        received_fields: vec![("email".into(), "Email".into(), "bob@example.com".into())],
        my_visible_fields: vec!["Phone".into()],
        group_names: Vec::new(),
    });
    let screen = engine.build_screen();
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::FieldList { id, .. } if id == "received_fields"
        )),
        "rich success screen must render the received card fields",
    );
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::InfoPanel { id, .. } if id == "my_visibility"
        )),
        "rich success screen must render the visibility section",
    );
}

fn engine_with_qr(state: ProtocolState, data: &str) -> MultiStageExchangeEngine {
    let mut e = MultiStageExchangeEngine::new_glance();
    e.set_state(state);
    e.set_qr_payload(&QrPayload {
        data: data.into(),
        error_correction: "L".into(),
        display_duration_ms: 400,
    });
    e
}

fn first_status_indicator(screen: &ScreenModel) -> Option<&Component> {
    screen
        .components
        .iter()
        .find(|c| matches!(c, Component::StatusIndicator { .. }))
}

fn action_ids(screen: &ScreenModel) -> Vec<&str> {
    // Screen-level actions (success / failed terminals) plus the
    // buttons the active screen now carries inside its preview
    // `Row`'s `ActionList` (so they sit beside the camera preview).
    let mut ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    fn collect<'a>(component: &'a Component, out: &mut Vec<&'a str>) {
        match component {
            Component::ActionList { items, .. } => {
                out.extend(items.iter().map(|i| i.id.as_str()));
            }
            Component::Row { items, .. } => {
                for child in items {
                    collect(child, out);
                }
            }
            _ => {}
        }
    }
    for c in &screen.components {
        collect(c, &mut ids);
    }
    ids
}

/// Pull the switch-camera button label out of the active screen's
/// preview `Row` `ActionList`.
fn switch_camera_label(screen: &ScreenModel) -> String {
    fn dig(c: &Component) -> Option<String> {
        match c {
            Component::ActionList { items, .. } => items
                .iter()
                .find(|i| i.id == SWITCH_CAMERA_ACTION_ID)
                .map(|i| i.label.clone()),
            Component::Row { items, .. } => items.iter().find_map(dig),
            _ => None,
        }
    }
    screen
        .components
        .iter()
        .find_map(dig)
        .expect("switch_camera button must exist")
}

/// Find the peer-scan `QrCode` wherever it lives (top-level or inside
/// the active screen's preview `Row`).
fn find_peer_scan(screen: &ScreenModel) -> Option<&Component> {
    fn dig(c: &Component) -> Option<&Component> {
        match c {
            Component::QrCode {
                id,
                mode: QrMode::Scan,
                ..
            } if id == PEER_SCAN_COMPONENT_ID => Some(c),
            Component::Row { items, .. } => items.iter().find_map(dig),
            _ => None,
        }
    }
    screen.components.iter().find_map(dig)
}

// ── Scan-stability layout (2026-06-03-exchange-qr-scan-stability) ──

// The active screen is a fixed (non-scrolling) layout so the own-QR
// never reflows while a live element updates — a moving QR breaks the
// peer camera's lock.
// @internal
#[test]
fn active_screen_layout_is_fixed() {
    let screen = engine_with_qr(ProtocolState::Advertising, "payload").current_screen();
    assert_eq!(screen.screen_id, SCREEN_ID);
    assert_eq!(screen.layout, ScreenLayout::Fixed);
    assert!(
        screen.requires_poll,
        "multi_stage_exchange must ask the shell for poll ticks (I4)"
    );
    assert!(
        !screen.requires_animated_qr,
        "multi_stage_exchange advances via poll, not animated QR frames"
    );
}

// The peer-scan preview and the buttons share one `Row`; the buttons
// live in that row's `ActionList` (not the screen-level `actions`).
// @internal
#[test]
fn active_screen_groups_preview_and_actions_in_row() {
    let screen = engine_with_qr(ProtocolState::Advertising, "payload").current_screen();
    assert!(
        screen.actions.is_empty(),
        "active screen actions must be empty; buttons live in the row"
    );
    let row = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Row { id, items } if id == EXCHANGE_PREVIEW_ROW_ID => Some(items),
            _ => None,
        })
        .expect("active screen must have the preview Row");
    assert!(
        row.iter().any(|c| matches!(
            c,
            Component::QrCode { id, mode: QrMode::Scan, .. } if id == COMPONENT_ID_PEER_SCAN
        )),
        "row must contain the peer-scan preview"
    );
    let button_ids: Vec<&str> = row
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == EXCHANGE_ACTIONS_ID => Some(items),
            _ => None,
        })
        .expect("row must contain the action list")
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(button_ids, vec![SWITCH_CAMERA_ACTION_ID, CANCEL_ACTION_ID]);
}

// Buttons now dispatch via `ListItemSelected` (ActionList); the engine
// normalises those back to the same handler as the old `ActionPressed`.
// @internal
#[test]
fn list_item_selected_on_action_list_toggles_camera() {
    let mut engine = engine_with_qr(ProtocolState::Advertising, "payload");
    let before = engine.use_front_camera();
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: EXCHANGE_ACTIONS_ID.into(),
        item_id: SWITCH_CAMERA_ACTION_ID.into(),
    });
    assert_ne!(
        engine.use_front_camera(),
        before,
        "switch_camera via ActionList must toggle the camera"
    );
    assert!(matches!(result, ActionResult::Commands { .. }));
}

// ── Mode-aware construction (Phase 1.A) ─────────────────────

// RED for Phase 1.A.2 of `2026-05-11-hover-graduation-plan.md`.
// Hover defaults the camera selector to `front` (face-to-face
// screen-to-screen) and starts with audio proximity `Pending`
// because the ultrasonic handshake hasn't run yet. The Glance
// path (`new_glance`) ignores `audio_proximity` and stays
// back-camera-default.
// @internal
#[test]
fn new_hover_initialises_state() {
    let engine = MultiStageExchangeEngine::new_hover();
    assert!(
        engine.use_front_camera(),
        "Hover engine must default to the front camera",
    );
    assert_eq!(
        engine.audio_proximity(),
        AudioProximityState::Pending,
        "Hover engine must start with audio proximity Pending",
    );
}

// @internal
#[test]
fn new_glance_is_back_camera_default() {
    let engine = MultiStageExchangeEngine::new_glance();
    assert!(
        !engine.use_front_camera(),
        "Glance engine must default to the back camera",
    );
}

// @internal
#[test]
fn is_hover_mode_reflects_constructor() {
    // Phase 1.C polish — the platform-binding wire-up reads
    // `is_hover_mode()` through `AppEngine::
    // is_active_engine_multi_stage_hover` to decide whether to
    // register the cycle-thread audio listener. Both
    // constructors must carry an honest mode marker.
    assert!(
        MultiStageExchangeEngine::new_hover().is_hover_mode(),
        "new_hover must mark the engine as Hover-mode",
    );
    assert!(
        !MultiStageExchangeEngine::new_glance().is_hover_mode(),
        "new_glance must NOT be Hover-mode (the legacy Glance flow has no audio handshake)",
    );
}

// @internal
#[test]
fn new_tap_hover_shake_initialises_state() {
    let engine = MultiStageExchangeEngine::new_tap_hover_shake();
    assert!(
        engine.use_front_camera(),
        "TapHoverShake engine must default to the front camera",
    );
    assert_eq!(engine.audio_proximity(), AudioProximityState::Pending);
    assert_eq!(
        engine.accel_proximity(),
        AccelerometerProximityState::Pending
    );
    assert!(
        engine.is_hover_mode(),
        "TapHoverShake runs the audio handshake, so the audio-listener marker must be set",
    );
}

// ── Audio-proximity setter + rendering (Phase 1.C.2 + 1.D) ────

// @internal
#[test]
fn set_audio_proximity_transitions_state() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    assert_eq!(engine.audio_proximity(), AudioProximityState::Pending);
    engine.set_audio_proximity(AudioProximityState::Listening);
    assert_eq!(engine.audio_proximity(), AudioProximityState::Listening);
    engine.set_audio_proximity(AudioProximityState::Confirmed);
    assert_eq!(engine.audio_proximity(), AudioProximityState::Confirmed);
}

// @internal
#[test]
fn set_audio_proximity_is_noop_when_cancelled() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    // Drive into the cancelled state via the same path the engine's
    // user-action handler uses — pressing CANCEL flips the flag.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: CANCEL_ACTION_ID.into(),
    });
    // Subsequent setter calls must not update the field; the
    // engine ignores late callbacks after the user cancelled.
    engine.set_audio_proximity(AudioProximityState::Confirmed);
    assert_eq!(
        engine.audio_proximity(),
        AudioProximityState::Pending,
        "cancelled engine must reject set_audio_proximity",
    );
}

// Proximity (audio/accel) narration was removed from the active
// screen's status; the own-QR label now carries the protocol-state
// caption and no longer reflects proximity progress. The former
// status-detail narration tests for Listening/Confirmed/Pending were
// deleted because they asserted removed behavior.

// @internal
#[test]
fn audio_proximity_failed_renders_audio_failed_screen() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    engine.set_audio_proximity(AudioProximityState::Failed);
    let screen = engine.current_screen();
    let status = first_status_indicator(&screen).expect("status indicator");
    let Component::StatusIndicator {
        title: status_title,
        ..
    } = status
    else {
        panic!("expected StatusIndicator");
    };
    assert_eq!(
        status_title, "Couldn't confirm devices are close",
        "audio-Failed must render the proximity-specific chrome, not the generic Exchange Failed panel",
    );
    // Retry + Cancel actions are present on the audio-failed
    // screen so the user can attempt the handshake again.
    let ids: Vec<&str> = action_ids(&screen);
    assert!(
        ids.contains(&RETRY_ACTION_ID),
        "audio-failed screen must offer Retry; got {ids:?}",
    );
    assert!(
        ids.contains(&CANCEL_ACTION_ID),
        "audio-failed screen must offer Cancel; got {ids:?}",
    );
}

// @internal
#[test]
fn audio_failed_takes_precedence_over_protocol_failed() {
    // Both failure modes co-exist on a single engine after a
    // failed handshake: protocol may have failed for an
    // unrelated reason while audio_proximity also went Failed.
    // The user-facing chrome should narrate the audio failure
    // (the actionable physical-setup hint) rather than a
    // generic "Exchange failed" panel.
    let mut engine = MultiStageExchangeEngine::new_hover();
    engine.set_state(ProtocolState::Failed("generic-reason".to_string()));
    engine.set_audio_proximity(AudioProximityState::Failed);
    let screen = engine.current_screen();
    let status = first_status_indicator(&screen).expect("status indicator");
    let Component::StatusIndicator {
        title: status_title,
        ..
    } = status
    else {
        panic!("expected StatusIndicator");
    };
    assert_eq!(
        status_title, "Couldn't confirm devices are close",
        "audio_proximity:Failed must take precedence over ProtocolState::Failed",
    );
}

// ── Accelerometer-proximity setter + rendering (P2.B) ────────
//
// TapHoverShake's second parallel proximity signal. Mirrors the
// audio-proximity suite above: a setter, status-detail hints, and a
// distinct Failed screen. Glance and Hover leave accel_proximity at
// Pending so their rendering is unchanged.

// @internal
#[test]
fn new_engines_initialise_accel_pending() {
    assert_eq!(
        MultiStageExchangeEngine::new_glance().accel_proximity(),
        AccelerometerProximityState::Pending,
        "Glance engine must start with accel proximity Pending",
    );
    assert_eq!(
        MultiStageExchangeEngine::new_hover().accel_proximity(),
        AccelerometerProximityState::Pending,
        "Hover engine must start with accel proximity Pending",
    );
}

// @internal
#[test]
fn set_accel_proximity_transitions_state() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    assert_eq!(
        engine.accel_proximity(),
        AccelerometerProximityState::Pending
    );
    engine.set_accel_proximity(AccelerometerProximityState::Listening);
    assert_eq!(
        engine.accel_proximity(),
        AccelerometerProximityState::Listening
    );
    engine.set_accel_proximity(AccelerometerProximityState::Confirmed);
    assert_eq!(
        engine.accel_proximity(),
        AccelerometerProximityState::Confirmed
    );
}

// @internal
#[test]
fn set_accel_proximity_is_noop_when_cancelled() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: CANCEL_ACTION_ID.into(),
    });
    engine.set_accel_proximity(AccelerometerProximityState::Confirmed);
    assert_eq!(
        engine.accel_proximity(),
        AccelerometerProximityState::Pending,
        "cancelled engine must reject set_accel_proximity",
    );
}

// The accel Listening/Confirmed status-detail narration tests were
// deleted alongside the audio ones: proximity narration no longer
// appears on the active screen.

// @internal
#[test]
fn accel_proximity_failed_renders_accel_failed_screen() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    engine.set_accel_proximity(AccelerometerProximityState::Failed);
    let screen = engine.current_screen();
    let status = first_status_indicator(&screen).expect("status indicator");
    let Component::StatusIndicator {
        title: status_title,
        ..
    } = status
    else {
        panic!("expected StatusIndicator");
    };
    assert_eq!(
        status_title, "Couldn't confirm the shake",
        "accel-Failed must render the shake-specific chrome, not the generic Exchange Failed panel",
    );
    let ids: Vec<&str> = action_ids(&screen);
    assert!(
        ids.contains(&RETRY_ACTION_ID),
        "accel-failed screen must offer Retry; got {ids:?}",
    );
    assert!(
        ids.contains(&CANCEL_ACTION_ID),
        "accel-failed screen must offer Cancel; got {ids:?}",
    );
}

// @internal
#[test]
fn accel_failed_takes_precedence_over_protocol_failed() {
    let mut engine = MultiStageExchangeEngine::new_hover();
    engine.set_state(ProtocolState::Failed("generic-reason".to_string()));
    engine.set_accel_proximity(AccelerometerProximityState::Failed);
    let screen = engine.current_screen();
    let status = first_status_indicator(&screen).expect("status indicator");
    let Component::StatusIndicator {
        title: status_title,
        ..
    } = status
    else {
        panic!("expected StatusIndicator");
    };
    assert_eq!(
        status_title, "Couldn't confirm the shake",
        "accel_proximity:Failed must take precedence over ProtocolState::Failed",
    );
}

// @internal
#[test]
fn audio_failed_takes_precedence_over_accel_failed() {
    // When both proximity signals fail, the audio hint wins the
    // single Failed screen — a deterministic, documented order
    // (audio branch is checked first in build_screen). The accel
    // failure is still recoverable via the shared Retry action.
    let mut engine = MultiStageExchangeEngine::new_hover();
    engine.set_audio_proximity(AudioProximityState::Failed);
    engine.set_accel_proximity(AccelerometerProximityState::Failed);
    let screen = engine.current_screen();
    let status = first_status_indicator(&screen).expect("status indicator");
    let Component::StatusIndicator {
        title: status_title,
        ..
    } = status
    else {
        panic!("expected StatusIndicator");
    };
    assert_eq!(
        status_title, "Couldn't confirm devices are close",
        "audio-Failed must win over accel-Failed on the single Failed screen",
    );
}

// ── Per-ProtocolState rendering ──────────────────────────────
// Split into a sibling file (M3 S5-10 tidy) to keep this file under
// the 1200-line test hard limit.
#[path = "multi_stage_exchange_protocol_state_tests.rs"]
mod protocol_state_tests;

// ── Camera gate ─────────────────────────────────────────────

// @internal
#[test]
fn permission_denied_event_swaps_to_permission_screen() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    let result = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "Camera".into(),
    });
    assert!(
        result.is_some(),
        "engine must update screen on permission denied"
    );
    let screen = engine.current_screen();
    let title_match = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { id, title, .. }
                if id == COMPONENT_ID_PERMISSION && title == "Camera Required",
        )
    });
    assert!(
        title_match,
        "permission denied must surface Camera Required"
    );
    assert_eq!(
        action_ids(&screen),
        vec![GRANT_CAMERA_PERMISSION_ACTION_ID, CANCEL_ACTION_ID],
    );
}

// @internal
#[test]
fn hardware_unavailable_event_swaps_to_hardware_screen() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "camera".into(),
    });
    let screen = engine.current_screen();
    let has_hardware = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { id, title, status: Status::Failed, .. }
                if id == COMPONENT_ID_HARDWARE && title == "Camera Unavailable",
        )
    });
    assert!(has_hardware);
    assert_eq!(action_ids(&screen), vec![CANCEL_ACTION_ID]);
}

// @internal
#[test]
fn unrelated_transport_does_not_engage_gate() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "BLE".into(),
    });
    // Still active rendering — the BLE permission denial does not
    // gate the camera-only flow.
    let screen = engine.current_screen();
    assert!(
        find_peer_scan(&screen).is_some(),
        "unrelated transport must not gate the camera screen"
    );
}

// ── Action handling ─────────────────────────────────────────

// @internal
#[test]
fn cancel_action_returns_complete_and_blocks_further_state_pushes() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: CANCEL_ACTION_ID.into(),
    });
    assert!(matches!(result, ActionResult::Complete));
    // Late state push from the cycle thread must not un-cancel.
    engine.set_state(ProtocolState::Finalized);
    engine.set_finalized("Late".into());
    // Engine still considers itself cancelled — state didn't move.
    assert_eq!(engine.state, ProtocolState::Idle);
    assert!(engine.peer_name.is_none());
}

// @internal
#[test]
fn done_action_returns_complete() {
    let mut engine = engine_with_state(ProtocolState::Finalized);
    engine.set_session_ended();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: DONE_ACTION_ID.into(),
    });
    assert!(matches!(result, ActionResult::Complete));
}

// @internal
#[test]
fn switch_camera_toggles_state_and_emits_command() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    assert!(!engine.use_front_camera());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: SWITCH_CAMERA_ACTION_ID.into(),
    });
    match result {
        ActionResult::Commands { commands } => match &commands[0] {
            vauchi_core::Command::SwitchCamera { use_front } => {
                assert!(use_front, "first toggle must select front");
            }
            other => panic!("expected SwitchCamera, got {other:?}"),
        },
        other => panic!("expected Commands, got {other:?}"),
    }
    assert!(engine.use_front_camera());
    // Toggle back.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: SWITCH_CAMERA_ACTION_ID.into(),
    });
    assert!(!engine.use_front_camera());
}

// @internal
#[test]
fn switch_camera_label_reflects_current_orientation() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    assert_eq!(
        switch_camera_label(&engine.current_screen()),
        "Use Front Camera"
    );
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: SWITCH_CAMERA_ACTION_ID.into(),
    });
    assert_eq!(
        switch_camera_label(&engine.current_screen()),
        "Use Rear Camera"
    );
}

// @internal
#[test]
fn grant_permission_action_clears_gate_and_re_requests_scan() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    assert_eq!(engine.camera_gate, CameraGate::PermissionDenied);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: GRANT_CAMERA_PERMISSION_ACTION_ID.into(),
    });
    assert_eq!(engine.camera_gate, CameraGate::Available);
    match result {
        ActionResult::Commands { commands } => {
            assert!(matches!(&commands[0], vauchi_core::Command::QrRequestScan,));
        }
        other => panic!("expected Commands, got {other:?}"),
    }
}

// @internal
#[test]
fn unavailable_gate_cannot_be_recovered_by_grant_permission() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "camera".into(),
    });
    // Permission-denied event arrives later — still terminal.
    engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    assert_eq!(engine.camera_gate, CameraGate::Unavailable);
    // Hardware screen has no Grant Permission affordance.
    let screen = engine.current_screen();
    let ids = action_ids(&screen);
    assert!(!ids.contains(&GRANT_CAMERA_PERMISSION_ACTION_ID));
}

// ── QrScanProgress hardware event ───────────────────────────

// @internal
#[test]
fn qr_scan_progress_drives_quality_tracker() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    for _ in 0..10 {
        engine.handle_hardware_event(Event::QrScanProgress {
            detected: true,
            confidence: Some(95),
            frame_skipped: false,
        });
    }
    let screen = engine.current_screen();
    let scan_quality = match find_peer_scan(&screen) {
        Some(Component::QrCode { scan_quality, .. }) => Some(*scan_quality),
        _ => None,
    };
    assert_eq!(scan_quality, Some(Some(ScanQuality::Good)));
}

// @internal
#[test]
fn skipped_frames_do_not_reach_tracker() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    // 10 detected frames — Good.
    for _ in 0..10 {
        engine.handle_hardware_event(Event::QrScanProgress {
            detected: true,
            confidence: None,
            frame_skipped: false,
        });
    }
    // Skipped frames must NOT pollute the rolling rate.
    for _ in 0..20 {
        engine.handle_hardware_event(Event::QrScanProgress {
            detected: false,
            confidence: None,
            frame_skipped: true,
        });
    }
    let screen = engine.current_screen();
    let scan_quality = match find_peer_scan(&screen) {
        Some(Component::QrCode { scan_quality, .. }) => *scan_quality,
        _ => None,
    };
    assert_eq!(scan_quality, Some(ScanQuality::Good));
}

// ── Adversarial input ───────────────────────────────────────

// @internal
#[test]
fn unknown_action_id_falls_through_to_update_screen() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "🦀;DROP TABLE".into(),
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));
}

// @internal
#[test]
fn non_action_pressed_user_action_falls_through() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "x".into(),
        item_id: "y".into(),
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));
}

// @internal
#[test]
fn long_failure_reason_renders_without_truncation() {
    let long = "a".repeat(1024);
    let engine = engine_with_state(ProtocolState::Failed(long.clone()));
    match first_status_indicator(&engine.current_screen()).unwrap() {
        Component::StatusIndicator { detail, .. } => {
            assert_eq!(detail.as_deref(), Some(long.as_str()));
        }
        _ => unreachable!(),
    }
}

// ── Screen-presentation lifecycle (Phase 2b) ─────────────────────

// @scenario: exchange.feature :: Multi-stage exchange (Glance) dims screen, disables idle timer, locks portrait, and announces back camera on entry
#[test]
fn screen_entered_glance_emits_presentation_commands_and_back_camera() {
    use vauchi_core::{Command, Orientation};
    let mut engine = MultiStageExchangeEngine::new_glance();
    let commands = engine.screen_entered();
    assert_eq!(
        commands,
        vec![
            Command::SetScreenBrightness { level: Some(0.65) },
            Command::SetIdleTimerDisabled { disabled: true },
            Command::SetOrientationLock {
                orientation: Some(Orientation::Portrait)
            },
            Command::SwitchCamera { use_front: false },
        ],
        "Glance screen_entered must dim brightness, disable idle timer, lock portrait, and announce back camera"
    );
}

// @scenario: exchange.feature :: Multi-stage exchange (Hover) dims screen, disables idle timer, locks portrait, and announces front camera on entry
#[test]
fn screen_entered_hover_emits_presentation_commands_and_front_camera() {
    use vauchi_core::{Command, Orientation};
    let mut engine = MultiStageExchangeEngine::new_hover();
    let commands = engine.screen_entered();
    assert_eq!(
        commands,
        vec![
            Command::SetScreenBrightness { level: Some(0.65) },
            Command::SetIdleTimerDisabled { disabled: true },
            Command::SetOrientationLock {
                orientation: Some(Orientation::Portrait)
            },
            Command::SwitchCamera { use_front: true },
        ],
        "Hover screen_entered must dim brightness, disable idle timer, lock portrait, and announce front camera"
    );
}

// @scenario: exchange.feature :: Multi-stage exchange restores presentation defaults on exit
#[test]
fn screen_exited_emits_brightness_idle_timer_and_orientation_unlock() {
    use vauchi_core::Command;
    let mut engine = MultiStageExchangeEngine::new_glance();
    let commands = engine.screen_exited();
    assert_eq!(
        commands,
        vec![
            Command::SetScreenBrightness { level: None },
            Command::SetIdleTimerDisabled { disabled: false },
            Command::SetOrientationLock { orientation: None },
        ],
        "screen_exited must restore brightness, idle timer, and orientation defaults"
    );
}

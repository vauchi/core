// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `ble_engine.rs` — extracted to keep the engine
//! file under the src size limit. Loaded via `#[path]`; stays a unit-test
//! child module (private-item access preserved).

// INLINE_TEST_REQUIRED: the engine wraps private flow state; tests drive it via
// the public WorkflowEngine surface + the screen/action ids.
use super::*;

fn discover(engine: &mut BleExchangeEngine) -> Option<ActionResult> {
    // Peer advertises a non-empty token; this engine's default
    // (empty) token sorts smaller, so it wins the tiebreak and
    // initiates the connection.
    engine.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "d1".into(),
        rssi: -40,
        adv_data: vec![0x01],
    })
}

// @internal
#[test]
fn new_engine_renders_discovering_and_not_cancelled() {
    let engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );
    assert!(!engine.was_cancelled());
}

// @internal
#[test]
fn glance_active_screen_shows_injected_qr_and_scan() {
    let engine = BleExchangeEngine::new(
        ExchangeMode::Glance,
        true,
        vec![1, 2, 3],
        SystemClock::shared(),
        Some("QR-PAYLOAD".to_string()),
        Locale::English,
    );
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_ble_glance");
    let own_qr = screen.components.iter().find_map(|c| match c {
        Component::QrCode {
            data,
            mode: QrMode::Display,
            ..
        } => Some(data.clone()),
        _ => None,
    });
    assert_eq!(
        own_qr.as_deref(),
        Some("QR-PAYLOAD"),
        "the displayed QR must carry the AppEngine-injected payload verbatim"
    );
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::QrCode {
                mode: QrMode::Scan,
                ..
            }
        )),
        "a camera device must offer a scan component"
    );
}

// @internal
#[test]
fn glance_without_camera_shows_qr_but_no_scan() {
    let engine = BleExchangeEngine::new(
        ExchangeMode::Glance,
        false,
        vec![9],
        SystemClock::shared(),
        Some("Q".to_string()),
        Locale::English,
    );
    let screen = engine.current_screen();
    assert!(
        screen.components.iter().any(|c| matches!(
            c,
            Component::QrCode {
                mode: QrMode::Display,
                ..
            }
        )),
        "own QR is always shown"
    );
    assert!(
        !screen.components.iter().any(|c| matches!(
            c,
            Component::QrCode {
                mode: QrMode::Scan,
                ..
            }
        )),
        "no camera → no scan component"
    );
}

// @internal
#[test]
fn stalled_step_past_timeout_ticks_to_failed() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    // `entered` read just after construction is >= the engine's stamped
    // step-entry second, so `+ budget + 1` is unambiguously past the
    // deadline (CC-06 — explicit now, no FakeClock, no sleep).
    let entered = SystemClock::shared().unix_seconds();
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );

    engine.tick(entered + BLE_STEP_TIMEOUT_SECS + 1);

    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_failed",
        "a stalled BLE step past its budget must fail to retry/cancel"
    );
}

// @internal
#[test]
fn step_within_timeout_stays_active() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let entered = SystemClock::shared().unix_seconds();

    engine.tick(entered);

    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering",
        "must not fail before the step budget elapses"
    );
}

// @internal
#[test]
fn tick_on_terminal_screen_is_inert() {
    // A tick far past any budget must not mutate a terminal screen
    // (the `screen != Active` guard, CC-14 adversarial case).
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    engine.force_failure(Some("crypto failure".into()));
    let before = engine.current_screen();

    engine.tick(u64::MAX);

    assert_eq!(engine.current_screen().screen_id, before.screen_id);
    assert_eq!(
        engine.current_screen().components,
        before.components,
        "tick must not mutate a terminal BLE screen"
    );
}

// @internal
#[test]
fn screen_entered_emits_advertise_then_scan_once() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Bump,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let cmds = engine.screen_entered();
    assert_eq!(cmds.len(), 2);
    assert!(matches!(cmds[0], Command::BleStartAdvertising { .. }));
    assert!(matches!(cmds[1], Command::BleStartScanning { .. }));
    // idempotent — no re-emit on the next render
    assert!(engine.screen_entered().is_empty());
}

// @scenario: contact_exchange.feature :: Glance selects its scan camera before BLE bootstrap
#[test]
fn glance_screen_entered_selects_rear_camera_before_ble_bootstrap() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Glance,
        true,
        vec![],
        SystemClock::shared(),
        Some("QR-PAYLOAD".to_string()),
        Locale::English,
    );

    let commands = engine.screen_entered();

    assert_eq!(commands.len(), 3);
    assert!(matches!(
        commands[0],
        Command::SwitchCamera { use_front: false }
    ));
    assert!(matches!(commands[1], Command::BleStartAdvertising { .. }));
    assert!(matches!(commands[2], Command::BleStartScanning { .. }));
    assert!(
        engine.screen_entered().is_empty(),
        "Glance must not restart camera or BLE hardware on re-render"
    );
}

// @internal
#[test]
fn discovery_event_emits_connect_command_and_advances() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let result = discover(&mut engine).expect("an active engine handles BLE events");
    match result {
        ActionResult::Commands { commands } => assert!(matches!(
            &commands[0],
            Command::BleConnect { device_id } if device_id == "d1"
        )),
        other => panic!("expected Commands, got {other:?}"),
    }
    assert_eq!(engine.current_screen().screen_id, "exchange_ble_exchanging");
}

// @internal
// F0 backoff: a radio responder (larger token) that discovers the peer but
// is never connected to (asymmetric discovery) dials out itself once the
// fallback window elapses, so the exchange self-heals instead of
// deadlocking. Role-by-direction then makes the outbound side the initiator.
#[test]
fn responder_backoff_dials_out_after_fallback_window() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![0xFF], // large own token → we are the responder vs peer 0x01
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let entered = SystemClock::shared().unix_seconds();
    // Discover a peer: as the responder we advance but emit no connect.
    let result = engine.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer1".into(),
        rssi: -50,
        adv_data: vec![0x01],
    });
    match result {
        Some(ActionResult::UpdateScreen(_)) => {}
        other => panic!("responder discovery emits no connect command, got {other:?}"),
    }
    // Before the window: no fallback.
    assert!(
        engine
            .tick(entered + BLE_FALLBACK_CONNECT_SECS - 1)
            .is_empty(),
        "no fallback connect before the backoff window",
    );
    // Past the window: dial out to the discovered peer.
    let cmds = engine.tick(entered + BLE_FALLBACK_CONNECT_SECS + 1);
    assert!(
        matches!(&cmds[0], Command::BleConnect { device_id } if device_id == "peer1"),
        "responder backoff dials out to the discovered peer: {cmds:?}",
    );
    // Idempotent — the fallback is emitted at most once.
    assert!(
        engine
            .tick(entered + BLE_FALLBACK_CONNECT_SECS + 2)
            .is_empty(),
        "fallback connect must fire only once",
    );
}

// @internal
#[test]
fn disconnect_transitions_to_failed_with_all_fallbacks() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Shake,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let _ = engine.handle_hardware_event(Event::BleDisconnected {
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
        reason: "lost".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_failed");
    let ids: Vec<&str> = screen
        .contextual_actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert!(ids.contains(&"retry"));
    assert!(ids.contains(&"fallback_qr")); // has_camera == true
    assert!(ids.contains(&"fallback_relay"));
    assert!(ids.contains(&"cancel"));
}

// @internal
#[test]
fn no_qr_fallback_offered_without_camera() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        false,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let _ = engine.handle_hardware_event(Event::BleDisconnected {
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
        reason: "x".into(),
    });
    let ids: Vec<String> = engine
        .current_screen()
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert!(!ids.iter().any(|i| i == "fallback_qr"));
    assert!(ids.iter().any(|i| i == "retry"));
}

// @internal
#[test]
fn force_success_flips_chrome_to_success_screen() {
    // P4: the real `BleHandshakeMachine` completion drives the chrome
    // to Success (the hollow flow no longer self-completes).
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );
    engine.force_success(None);
    assert_eq!(engine.current_screen().screen_id, "exchange_success");
}

// @internal
#[test]
fn cancel_completes_and_marks_cancelled() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
    assert!(engine.was_cancelled());
}

// @internal
#[test]
fn retry_from_failed_resets_to_active_and_re_emits_start() {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Bump,
        true,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let _ = engine.handle_hardware_event(Event::BleDisconnected {
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
        reason: "x".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "exchange_failed");
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );
    assert!(!engine.was_cancelled());
    assert_eq!(engine.screen_entered().len(), 2);
}

fn failed_engine(has_camera: bool) -> BleExchangeEngine {
    let mut engine = BleExchangeEngine::new(
        ExchangeMode::Magic,
        has_camera,
        vec![],
        SystemClock::shared(),
        None,
        Locale::English,
    );
    let _ = engine.handle_hardware_event(Event::BleDisconnected {
        device_id: "peer-1".into(),
        direction: vauchi_core::BleLinkDirection::Outbound,
        reason: "lost".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "exchange_failed");
    engine
}

// @scenario: exchange :: BLE failure falls back to Glance instead of cancelling
// @internal
#[test]
fn fallback_qr_from_failed_hands_off_to_glance_instead_of_cancelling() {
    let mut engine = failed_engine(true);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "fallback_qr".into(),
    });
    assert!(
        matches!(
            result,
            ActionResult::StartBleExchange {
                mode: ExchangeMode::Glance
            }
        ),
        "fallback_qr must hand off to Glance, got {result:?}"
    );
    assert!(!engine.was_cancelled(), "a fallback is not a cancel");
}

// @scenario: exchange :: BLE failure falls back to Link instead of cancelling
// @internal
#[test]
fn fallback_relay_from_failed_hands_off_to_link_instead_of_cancelling() {
    let mut engine = failed_engine(false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "fallback_relay".into(),
    });
    assert!(
        matches!(result, ActionResult::StartLinkExchange),
        "fallback_relay must hand off to Link, got {result:?}"
    );
    assert!(!engine.was_cancelled(), "a fallback is not a cancel");
}

// @internal
#[test]
fn unknown_action_on_failed_screen_re_renders_instead_of_cancelling() {
    let mut engine = failed_engine(true);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonsense".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => assert_eq!(screen.screen_id, "exchange_failed"),
        other => panic!("unknown action must re-render the failed screen, got {other:?}"),
    }
    assert!(!engine.was_cancelled());
}

// ── Glance manual code entry ──────────────────────────────────────
// `2026-09-09-tui-cannot-ingest-peer-exchange-payload`: a camera-less
// device (TUI) and an automation harness (Maestro) need a core-owned text
// route for the peer's Glance code.

fn glance_engine(has_camera: bool) -> BleExchangeEngine {
    BleExchangeEngine::new(
        ExchangeMode::Glance,
        has_camera,
        vec![],
        SystemClock::shared(),
        Some("OWN-QR".to_string()),
        Locale::English,
    )
}

fn code_input_of(screen: &ScreenModel) -> Option<(String, Option<String>, String)> {
    screen.components.iter().find_map(|c| match c {
        Component::TextInput {
            id,
            label,
            placeholder,
            value,
            ..
        } if id == GLANCE_CODE_INPUT_ID => {
            Some((label.clone(), placeholder.clone(), value.clone()))
        }
        _ => None,
    })
}

fn has_scan_component(screen: &ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(
            c,
            Component::QrCode {
                mode: QrMode::Scan,
                ..
            }
        )
    })
}

fn action_label(screen: &ScreenModel, id: &str) -> Option<String> {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.label.clone())
}

fn valid_glance_code() -> String {
    let now = SystemClock::shared().unix_seconds();
    let identity = vauchi_core::identity::Identity::create("Peer", now);
    vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::generate(
        &identity,
        &identity.x3dh_keypair(),
        now,
    )
    .to_data_string()
}

fn submit_code(engine: &mut BleExchangeEngine, code: &str) -> ActionResult {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: GLANCE_CODE_INPUT_ID.into(),
        value: code.into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: ACTION_CONNECT_CODE.into(),
    })
}

// @scenario: contact_exchange.feature :: Glance without a camera accepts the peer code as text
#[test]
fn glance_without_a_camera_renders_the_manual_code_input() {
    let engine = glance_engine(false);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "exchange_ble_glance");
    assert_eq!(
        code_input_of(&screen),
        Some((
            "Paste the exchange data from another user".to_string(),
            Some("Paste or type the code".to_string()),
            String::new(),
        )),
        "no camera → the peer code is entered as text"
    );
    assert!(
        !has_scan_component(&screen),
        "no camera → no scan component"
    );
    assert_eq!(
        action_label(&screen, ACTION_CONNECT_CODE).as_deref(),
        Some("Connect")
    );
    assert_eq!(
        action_label(&screen, ACTION_ENTER_CODE),
        None,
        "the input is already shown — nothing to switch to"
    );
}

// @scenario: contact_exchange.feature :: Glance with a camera offers manual code entry
#[test]
fn glance_with_a_camera_offers_enter_code_and_switches_to_the_input() {
    let mut engine = glance_engine(true);
    let screen = engine.current_screen();
    assert!(has_scan_component(&screen));
    assert_eq!(code_input_of(&screen), None, "camera → scan first");
    assert_eq!(
        action_label(&screen, ACTION_ENTER_CODE).as_deref(),
        Some("Enter Code Manually")
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: ACTION_ENTER_CODE.into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("enter_code must re-render the glance screen, got {result:?}");
    };
    assert_eq!(screen.screen_id, "exchange_ble_glance");
    assert_eq!(
        code_input_of(&screen).map(|(_, placeholder, _)| placeholder),
        Some(Some("Paste or type the code".to_string()))
    );
    assert!(
        !has_scan_component(&screen),
        "the input step replaces the camera"
    );
    assert_eq!(
        action_label(&screen, ACTION_CONNECT_CODE).as_deref(),
        Some("Connect")
    );
}

// @internal
#[test]
fn typed_code_is_echoed_back_in_the_input() {
    let mut engine = glance_engine(false);
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: GLANCE_CODE_INPUT_ID.into(),
        value: "partial".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("typing re-renders, got {result:?}");
    };
    assert_eq!(
        code_input_of(&screen).map(|(_, _, value)| value),
        Some("partial".to_string())
    );
    assert_eq!(
        engine.engine_output(),
        None,
        "typing alone must not hand a code to the AppEngine"
    );
}

// @scenario: contact_exchange.feature :: Glance without a camera accepts the peer code as text
#[test]
fn submitting_a_peer_code_is_handled_like_a_scan() {
    let mut engine = glance_engine(false);
    let code = valid_glance_code();

    let result = submit_code(&mut engine, &code);

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("an accepted code waits for discovery, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "exchange_ble_discovering",
        "an accepted code moves on to the BLE wait, like a successful scan"
    );
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::GlancePeerCode { data: code }),
        "the AppEngine pins the peer from the submitted code exactly as from a scan"
    );
}

// @internal
#[test]
fn text_submitted_on_the_code_input_commits_like_connect() {
    let mut engine = glance_engine(true);
    let code = valid_glance_code();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: ACTION_ENTER_CODE.into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: GLANCE_CODE_INPUT_ID.into(),
        value: code.clone(),
    });

    let _ = engine.handle_action(UserAction::TextSubmitted {
        component_id: GLANCE_CODE_INPUT_ID.into(),
    });

    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::GlancePeerCode { data: code })
    );
}

// @scenario: contact_exchange.feature :: A malformed peer code fails to the retry screen
#[test]
fn submitting_garbage_lands_on_the_retry_screen() {
    let mut engine = glance_engine(false);
    let now = SystemClock::shared().unix_seconds();
    let expected_detail =
        vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::verified_from_data_string(
            "not a vauchi code",
            now,
        )
        .expect_err("garbage must not parse")
        .user_message()
        .to_string();

    let result = submit_code(&mut engine, "not a vauchi code");

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("garbage must render the failed screen, got {result:?}");
    };
    assert_eq!(screen.screen_id, "exchange_failed");
    let detail = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator { detail, .. } => Some(detail.clone()),
        _ => None,
    });
    assert_eq!(detail, Some(Some(expected_detail)));
    assert_eq!(
        action_label(&screen, ACTION_RETRY).as_deref(),
        Some("Retry")
    );
    assert_eq!(
        engine.engine_output(),
        None,
        "a rejected code must not be handed to the AppEngine"
    );
}

// @internal
#[test]
fn retry_after_a_bad_code_returns_to_the_code_input() {
    let mut engine = glance_engine(true);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: ACTION_ENTER_CODE.into(),
    });
    let _ = submit_code(&mut engine, "");
    assert_eq!(engine.current_screen().screen_id, "exchange_failed");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: ACTION_RETRY.into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_ble_glance");
    assert_eq!(
        code_input_of(&screen).map(|(_, _, value)| value),
        Some(String::new()),
        "retry keeps the user on manual entry with a cleared input"
    );
}

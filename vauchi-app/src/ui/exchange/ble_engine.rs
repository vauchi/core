// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dedicated core-driven Humble engine for the BLE exchange modes
//! (`ExchangeMode::Magic` / `Bump` / `Shake`).
//!
//! Graduation Slice 1 (`_private/docs/problems/2026-05-11-ble-exchange-engine-graduation/`):
//! a `WorkflowEngine` that **wraps** the existing, well-tested
//! [`super::ble::BleExchangeFlow`] state machine rather than rewriting it. The
//! engine owns the flow, renders its screens (reusing the `build_*_screen`
//! builders), maps the flow's `BleHardwareOutcome`s to `ActionResult`s (the
//! logic lifted verbatim from the legacy `ExchangeEngine::apply_ble_outcome`),
//! and emits the initial advertise/scan commands on screen entry (ADR-031:
//! commands out, events in; the flow never touches hardware).
//!
//! Mirrors `LinkExchangeEngine` / `MultiStageExchangeEngine`. The mode-dispatch
//! entry (`ActionResult::StartBleExchange`) + the legacy parent-arm retirement
//! land in slices 2–3.

use crate::ui::*;
use std::sync::Arc;
use vauchi_core::clock::Clock;
#[cfg(test)]
use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::{Command, Event};

use super::ble::{
    BleExchangeFlow, BleHardwareOutcome, BleStep, build_discovering_screen,
    build_exchanging_screen, build_verifying_screen, handle_ble_action,
};

/// Action id for the Cancel button (any screen).
pub const ACTION_CANCEL: &str = "cancel";
/// Action id for the Retry button on the failed screen.
pub const ACTION_RETRY: &str = "retry";
/// Action id for the Done button on the success screen.
pub const ACTION_DONE: &str = "done";

/// How long a non-terminal BLE step (`Discovering`/`Handshaking`/
/// `Exchanging`/`Verifying`) may persist with no progress before the
/// engine fails to the retry/cancel screen. Re-stamped on every forward
/// `BleStep` transition AND every consumed in-step event (e.g. a transfer
/// chunk while `Exchanging`), so a healthy exchange never trips it; only a
/// step with no inbound progress for the budget does — no peer discovered,
/// or a peer that connects then goes silent
/// (`2026-06-11-exchange-waits-forever-without-capabilities`,
/// T1.2; ADR-021: core owns the timer). Unix-seconds (the engine's
/// clock domain). Phase 0 (android!523) already handles permission-denied
/// fast; this is the no-event-ever backstop.
pub const BLE_STEP_TIMEOUT_SECS: u64 = 60;

/// Presentation state of the BLE engine. The active sub-flow screen is derived
/// from the wrapped flow's `BleStep`; `Success`/`Failed` are terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BleScreen {
    Active,
    Success,
    Failed { reason: Option<String> },
}

/// Component id of this device's Glance own-QR (display mode).
const GLANCE_OWN_QR_COMPONENT_ID: &str = "glance_own_qr";

/// Component id of the Glance peer-scan camera. A `UserAction::TextChanged` on
/// this id carries the scanned OOB QR; the platform facade routes it to
/// `AppEngine::apply_glance_scan` (the Cycle D contract).
pub const GLANCE_SCAN_COMPONENT_ID: &str = "glance_peer_scan";

/// Dedicated BLE exchange engine — wraps [`BleExchangeFlow`].
pub struct BleExchangeEngine {
    mode: ExchangeMode,
    flow: BleExchangeFlow,
    screen: BleScreen,
    /// QR fallback is offered on failure only when this device has a camera.
    has_camera: bool,
    /// `true` once the initial advertise/scan commands have been emitted, so
    /// `screen_entered` is idempotent across re-renders.
    started: bool,
    cancelled: bool,
    /// This device's role-tiebreak token, advertised in the
    /// `BleStartAdvertising.payload` so the peer can compare and exactly
    /// one side initiates the connection (see [`BleExchangeFlow`]).
    own_token: Vec<u8>,
    clock: Arc<dyn Clock>,
    /// Unix-seconds when the current `BleStep` was entered; re-stamped on
    /// every step transition (and on retry). The `tick` stall deadline
    /// ([`BLE_STEP_TIMEOUT_SECS`]) is measured from it.
    step_entered_unix: u64,
    /// Glance one-sided-QR payload this device displays, generated ONCE by the
    /// AppEngine on screen entry (a stable nonce for the whole attempt).
    /// `None` for radio modes (Magic/Bump/Shake).
    glance_qr: Option<String>,
}

impl BleExchangeEngine {
    /// Build a fresh BLE engine for `mode` (Magic/Bump/Shake). `has_camera`
    /// gates the QR fallback offer on the failed screen. `own_token` is this
    /// device's role-tiebreak token (a stable per-identity value); the engine
    /// advertises it and [`BleExchangeFlow`] compares it against the peer's to
    /// pick exactly one initiator.
    pub fn new(
        mode: ExchangeMode,
        has_camera: bool,
        own_token: Vec<u8>,
        clock: Arc<dyn Clock>,
        glance_qr: Option<String>,
    ) -> Self {
        let step_entered_unix = clock.unix_seconds();
        Self {
            mode,
            flow: BleExchangeFlow::new(mode, own_token.clone()),
            screen: BleScreen::Active,
            has_camera,
            started: false,
            cancelled: false,
            own_token,
            clock,
            step_entered_unix,
            glance_qr,
        }
    }

    /// Drive the chrome to the terminal Success screen. Called by the
    /// AppEngine when the real `BleHandshakeMachine` reports `Completed`:
    /// the hollow flow no longer self-completes from BLE data bytes (P4),
    /// so the real completion is what flips the UI to success.
    pub fn force_success(&mut self) {
        self.screen = BleScreen::Success;
    }

    /// Drive the chrome to the terminal Failed screen. Called by the
    /// AppEngine when the real `BleHandshakeMachine` reports `Failed`:
    /// a machine-level (crypto / protocol) failure has no hardware
    /// event for the hollow flow to observe, so without this the
    /// chrome rendered "Exchanging..." forever (P5b re-test,
    /// `2026-06-06-android-ble-execution`, 2026-06-10).
    pub fn force_failure(&mut self, reason: Option<String>) {
        self.screen = BleScreen::Failed { reason };
    }

    /// The advertise + scan commands that open a BLE exchange (lifted from the
    /// legacy `ExchangeEngine::start_ble_mode`).
    fn start_commands(&self) -> Vec<Command> {
        let service_uuid = vauchi_core::exchange::VAUCHI_BLE_SERVICE_UUID.to_string();
        vec![
            Command::BleStartAdvertising {
                service_uuid: service_uuid.clone(),
                payload: self.own_token.clone(),
            },
            Command::BleStartScanning { service_uuid },
        ]
    }

    /// Glance one-sided-QR active screen: this device's QR (to be scanned) plus
    /// a camera to scan the peer's. Shown while discovering; once connected the
    /// exchanging-progress screen takes over. The QR is display-only and the
    /// scan reports via `TextChanged` on [`GLANCE_SCAN_COMPONENT_ID`], so this
    /// screen adds no new action handler beyond `cancel`.
    fn build_glance_active_screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();
        if let Some(data) = &self.glance_qr {
            components.push(Component::QrCode {
                id: GLANCE_OWN_QR_COMPONENT_ID.into(),
                data: data.clone(),
                mode: QrMode::Display,
                label: Some("Show this to exchange".into()),
                scan_quality: None,
                a11y: None,
            });
        }
        if self.has_camera {
            components.push(Component::QrCode {
                id: GLANCE_SCAN_COMPONENT_ID.into(),
                data: String::new(),
                mode: QrMode::Scan,
                label: Some("Scan their code".into()),
                scan_quality: None,
                a11y: None,
            });
        }
        ScreenModel {
            screen_id: "exchange_ble_glance".into(),
            title: "Glance".into(),
            subtitle: Some("Show your code or scan theirs".into()),
            components,
            actions: vec![ScreenAction {
                id: "cancel".into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.screen {
            BleScreen::Active => match self.flow.step() {
                BleStep::Discovering if self.mode == ExchangeMode::Glance => {
                    self.build_glance_active_screen()
                }
                BleStep::Discovering => build_discovering_screen(self.mode),
                BleStep::Handshaking | BleStep::Exchanging => build_exchanging_screen(self.mode),
                BleStep::Verifying => build_verifying_screen(self.mode),
                // Complete is transitional — `apply_outcome` flips `screen` to
                // `Success` before this renders; show the exchanging screen if
                // it is ever observed mid-transition.
                BleStep::Complete => build_exchanging_screen(self.mode),
            },
            BleScreen::Success => self.build_success_screen(),
            BleScreen::Failed { reason } => self.build_failed_screen(reason.clone()),
        }
    }

    fn build_success_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_success".into(),
            title: "Success".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "success_status".into(),
                icon: None,
                title: "Exchange Complete".into(),
                detail: None,
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some("Exchange complete".into()),
                    hint: Some("Contact cards have been exchanged successfully".into()),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: ACTION_DONE.into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, detail: Option<String>) -> ScreenModel {
        let mut actions = vec![ScreenAction {
            id: ACTION_RETRY.into(),
            label: "Retry".into(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        }];
        if self.has_camera {
            actions.push(ScreenAction {
                id: "fallback_qr".into(),
                label: "Switch to QR".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y {
                    label: None,
                    hint: Some(
                        "Abandons this attempt and restarts the exchange using camera QR codes."
                            .into(),
                    ),
                    role: None,
                }),
            });
        }
        actions.push(ScreenAction {
            id: "fallback_relay".into(),
            label: "Switch to encrypted relay".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y {
                label: None,
                hint: Some(
                    "Abandons this attempt and completes the exchange over the encrypted relay server."
                        .into(),
                ),
                role: None,
            }),
        });
        actions.push(ScreenAction {
            id: ACTION_CANCEL.into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        ScreenModel {
            screen_id: "exchange_failed".into(),
            title: "Failed".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: "Exchange Failed".into(),
                detail,
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some("Exchange failed".into()),
                    hint: Some("The exchange did not complete. Retry or cancel.".into()),
                    role: None,
                }),
            }],
            actions,
            ..Default::default()
        }
    }

    /// Translate a `BleHardwareOutcome` to an `ActionResult` + state change.
    /// Lifted from the legacy `ExchangeEngine::apply_ble_outcome`.
    fn apply_outcome(&mut self, outcome: BleHardwareOutcome) -> ActionResult {
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands }
            | BleHardwareOutcome::Consumed { commands } => {
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            BleHardwareOutcome::Complete {
                card_bytes: _,
                commands,
            } => {
                // Card persistence is core-owned via the completion path; the
                // engine only flips to the terminal success screen.
                self.screen = BleScreen::Success;
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            BleHardwareOutcome::FailedWithFallback { reason } => {
                self.screen = BleScreen::Failed {
                    reason: Some(reason),
                };
                ActionResult::UpdateScreen(self.build_screen())
            }
            BleHardwareOutcome::Ignored => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

impl WorkflowEngine for BleExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    /// Emit the initial advertise/scan commands once, on first screen entry —
    /// the BLE equivalent of the legacy `start_ble_mode` entry commands.
    fn screen_entered(&mut self) -> Vec<Command> {
        if self.started || self.cancelled || self.screen != BleScreen::Active {
            return Vec::new();
        }
        self.started = true;
        self.start_commands()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Terminal screens.
        match &self.screen {
            BleScreen::Success => {
                return match action {
                    UserAction::ActionPressed { action_id } if action_id == ACTION_DONE => {
                        ActionResult::Complete
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                };
            }
            BleScreen::Failed { .. } => {
                return match action {
                    UserAction::ActionPressed { action_id } if action_id == ACTION_RETRY => {
                        // Fresh attempt: reset the wrapped flow and re-emit the
                        // start commands on the next `screen_entered`.
                        self.flow = BleExchangeFlow::new(self.mode, self.own_token.clone());
                        self.screen = BleScreen::Active;
                        self.started = false;
                        self.step_entered_unix = self.clock.unix_seconds();
                        ActionResult::UpdateScreen(self.build_screen())
                    }
                    UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                    // `fallback_qr` / `fallback_relay` switch transport — a
                    // router concern wired in slice 2; treated as cancel until
                    // then so the buttons never dead-end silently.
                    _ => {
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                };
            }
            BleScreen::Active => {}
        }

        if self.cancelled {
            return ActionResult::UpdateScreen(self.build_screen());
        }

        // Active sub-flow actions (cancel / fallback) — delegate to the flow's
        // action handler, then map the outcome.
        match handle_ble_action(self.flow.step(), &action) {
            Some(_) => {
                // Cancel and FallbackToRelay both end the BLE attempt; the
                // transport-switch (relay) is router-level (slice 2).
                self.cancelled = true;
                ActionResult::Complete
            }
            None => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        if !matches!(self.screen, BleScreen::Active) {
            return None;
        }
        let outcome = self.flow.handle_event(&event);
        // Any forward progress refreshes the stall deadline (T1.2): a step
        // advance OR a consumed in-step event — notably a transfer chunk
        // while `Exchanging` (a unit step that otherwise never re-stamps).
        // So the deadline only trips on a genuinely silent step, not a slow
        // transfer. `Ignored` (irrelevant event) and the terminal outcomes
        // do not re-stamp.
        if matches!(
            outcome,
            BleHardwareOutcome::StepAdvanced { .. } | BleHardwareOutcome::Consumed { .. }
        ) {
            self.step_entered_unix = self.clock.unix_seconds();
        }
        Some(self.apply_outcome(outcome))
    }

    /// Fail a stalled non-terminal BLE step past [`BLE_STEP_TIMEOUT_SECS`]
    /// (T1.2, ADR-021). Driven by the `poll_notifications` pump. `Active`
    /// implies a waiting step (`Complete` flips to `Success` via
    /// `apply_outcome`); `Success`/`Failed` are terminal.
    fn tick(&mut self, now: u64) {
        if self.cancelled || !matches!(self.screen, BleScreen::Active) {
            return;
        }
        if now.saturating_sub(self.step_entered_unix) >= BLE_STEP_TIMEOUT_SECS {
            self.force_failure(Some(
                "No nearby device responded — Bluetooth exchange timed out.".into(),
            ));
        }
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        match update {
            crate::ui::EngineUpdate::BleForceSuccess => {
                self.force_success();
                true
            }
            crate::ui::EngineUpdate::BleForceFailure { reason } => {
                self.force_failure(reason);
                true
            }
            _ => false,
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}

// INLINE_TEST_REQUIRED: the engine wraps private flow state; tests drive it via
// the public WorkflowEngine surface + the screen/action ids.
#[cfg(test)]
mod tests {
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
        );
        let cmds = engine.screen_entered();
        assert_eq!(cmds.len(), 2);
        assert!(matches!(cmds[0], Command::BleStartAdvertising { .. }));
        assert!(matches!(cmds[1], Command::BleStartScanning { .. }));
        // idempotent — no re-emit on the next render
        assert!(engine.screen_entered().is_empty());
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
    #[test]
    fn disconnect_transitions_to_failed_with_all_fallbacks() {
        let mut engine = BleExchangeEngine::new(
            ExchangeMode::Shake,
            true,
            vec![],
            SystemClock::shared(),
            None,
        );
        let _ = engine.handle_hardware_event(Event::BleDisconnected {
            reason: "lost".into(),
        });
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_failed");
        let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
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
        );
        let _ = engine.handle_hardware_event(Event::BleDisconnected { reason: "x".into() });
        let ids: Vec<String> = engine
            .current_screen()
            .actions
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
        );
        assert_eq!(
            engine.current_screen().screen_id,
            "exchange_ble_discovering"
        );
        engine.force_success();
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
        );
        let _ = engine.handle_hardware_event(Event::BleDisconnected { reason: "x".into() });
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
}

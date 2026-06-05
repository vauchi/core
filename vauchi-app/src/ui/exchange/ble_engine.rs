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

// Slice 1 lands the engine + its tests ahead of the router wiring. Slice 2
// constructs it via `ActionResult::StartBleExchange` and REMOVES this
// attribute, at which point `-D dead-code` re-verifies nothing is unused.
#![allow(dead_code)]

use crate::ui::*;
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

/// Presentation state of the BLE engine. The active sub-flow screen is derived
/// from the wrapped flow's `BleStep`; `Success`/`Failed` are terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BleScreen {
    Active,
    Success,
    Failed { reason: Option<String> },
}

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
}

impl BleExchangeEngine {
    /// Build a fresh BLE engine for `mode` (Magic/Bump/Shake). `has_camera`
    /// gates the QR fallback offer on the failed screen.
    pub fn new(mode: ExchangeMode, has_camera: bool) -> Self {
        Self {
            mode,
            flow: BleExchangeFlow::new(mode),
            screen: BleScreen::Active,
            has_camera,
            started: false,
            cancelled: false,
        }
    }

    fn progress(&self) -> Progress {
        Progress {
            current_step: self.flow.step().step_number(0),
            total_steps: BleStep::STEP_COUNT,
            label: None,
        }
    }

    /// The advertise + scan commands that open a BLE exchange (lifted from the
    /// legacy `ExchangeEngine::start_ble_mode`).
    fn start_commands() -> Vec<Command> {
        let service_uuid = vauchi_core::exchange::VAUCHI_BLE_SERVICE_UUID.to_string();
        vec![
            Command::BleStartAdvertising {
                service_uuid: service_uuid.clone(),
                payload: vec![],
            },
            Command::BleStartScanning { service_uuid },
        ]
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.screen {
            BleScreen::Active => match self.flow.step() {
                BleStep::Discovering => build_discovering_screen(self.mode, self.progress()),
                BleStep::Handshaking | BleStep::Exchanging => {
                    build_exchanging_screen(self.mode, self.progress())
                }
                BleStep::Verifying => build_verifying_screen(self.mode, self.progress()),
                // Complete is transitional — `apply_outcome` flips `screen` to
                // `Success` before this renders; show the exchanging screen if
                // it is ever observed mid-transition.
                BleStep::Complete => build_exchanging_screen(self.mode, self.progress()),
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
            progress: Some(self.progress()),
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
            progress: Some(self.progress()),
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
        Self::start_commands()
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
                        self.flow = BleExchangeFlow::new(self.mode);
                        self.screen = BleScreen::Active;
                        self.started = false;
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
        Some(self.apply_outcome(outcome))
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

// INLINE_TEST_REQUIRED: the engine wraps private flow state; tests drive it via
// the public WorkflowEngine surface + the screen/action ids.
#[cfg(test)]
mod tests {
    use super::*;

    fn discover(engine: &mut BleExchangeEngine) -> Option<ActionResult> {
        engine.handle_hardware_event(Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        })
    }

    // @internal
    #[test]
    fn new_engine_renders_discovering_and_not_cancelled() {
        let engine = BleExchangeEngine::new(ExchangeMode::Magic, true);
        assert_eq!(
            engine.current_screen().screen_id,
            "exchange_ble_discovering"
        );
        assert!(!engine.was_cancelled());
    }

    // @internal
    #[test]
    fn screen_entered_emits_advertise_then_scan_once() {
        let mut engine = BleExchangeEngine::new(ExchangeMode::Bump, true);
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
        let mut engine = BleExchangeEngine::new(ExchangeMode::Magic, true);
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
        let mut engine = BleExchangeEngine::new(ExchangeMode::Shake, true);
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
        let mut engine = BleExchangeEngine::new(ExchangeMode::Magic, false);
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
    fn cancel_completes_and_marks_cancelled() {
        let mut engine = BleExchangeEngine::new(ExchangeMode::Magic, true);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert!(engine.was_cancelled());
    }

    // @internal
    #[test]
    fn retry_from_failed_resets_to_active_and_re_emits_start() {
        let mut engine = BleExchangeEngine::new(ExchangeMode::Bump, true);
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

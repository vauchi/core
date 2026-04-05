// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE exchange sub-flow — screen builders and step logic for
//! Magic (BLE + audio), Bump (BLE + impact), and Shake (BLE +
//! accelerometer correlation) modes.
//!
//! Follows the `exchange_qr.rs` / `exchange_link.rs` pattern:
//! steps, screen builders, action/hardware-event handlers that
//! return outcomes for the parent engine to act on.

use crate::ui::*;
use vauchi_core::exchange::command::{ExchangeCommand, ExchangeHardwareEvent};
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::exchange::proximity_runner::{
    ProximityMethod, ProximityRunner, ProximityRunnerResult,
};

// ── Step enum ──────────────────────────────────────────────────────────────

/// Steps specific to the BLE exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // Variants scaffolded for Phase 1–3
pub(super) enum BleStep {
    /// Scanning for nearby BLE peers.
    Discovering,
    /// Running BLE handshake protocol (key exchange).
    Handshaking,
    /// Exchanging contact card data over BLE GATT.
    Exchanging,
    /// Running proximity verification (audio/impact/accel).
    Verifying,
    /// BLE exchange complete, results ready.
    Complete,
}

impl BleStep {
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::Discovering => 0,
            Self::Handshaking => 1,
            Self::Exchanging => 1,
            Self::Verifying => 2,
            Self::Complete => 2,
        }
    }

    /// Matches QrStep/LinkStep for consistent progress bar.
    pub(super) const STEP_COUNT: u8 = 3;
}

// ── Action/hardware outcomes ───────────────────────────────────────────────

/// Result of handling a user action in the BLE sub-flow.
#[allow(dead_code)] // Ignored variant used in Phase 1
pub(super) enum BleActionOutcome {
    /// No state change — action not handled by BLE flow.
    Ignored,
    /// User accepted relay fallback after BLE timeout.
    FallbackToRelay,
    /// User cancelled.
    Cancel,
}

/// Result of handling a hardware event in the BLE sub-flow.
#[derive(Debug)]
pub(super) enum BleHardwareOutcome {
    /// Step advanced — parent should update screen. May emit commands.
    StepAdvanced { commands: Vec<ExchangeCommand> },
    /// BLE exchange completed — card bytes available.
    Complete {
        card_bytes: Vec<u8>,
        commands: Vec<ExchangeCommand>,
    },
    /// BLE failed — offer relay fallback.
    FailedWithFallback { reason: String },
    /// Event consumed but no step change. May emit commands.
    Consumed { commands: Vec<ExchangeCommand> },
    /// Event not handled by BLE flow.
    Ignored,
}

// ── Screen builders ────────────────────────────────────────────────────────

pub(super) fn build_discovering_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let (title, subtitle) = match mode {
        ExchangeMode::Magic => (
            "Searching nearby...",
            "Hold your phone near the other device",
        ),
        ExchangeMode::Bump => ("Ready to bump", "Bump your phones together to exchange"),
        ExchangeMode::Shake => ("Ready to shake", "Shake both phones together to exchange"),
        _ => ("Searching...", "Looking for nearby devices"),
    };

    ScreenModel {
        screen_id: "exchange_ble_discovering".into(),
        title: title.into(),
        subtitle: Some(subtitle.into()),
        components: vec![Component::Text {
            id: "ble_status".into(),
            content: "Scanning for nearby devices...".into(),
            style: TextStyle::Body,
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

pub(super) fn build_exchanging_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let title = match mode {
        ExchangeMode::Magic => "Exchanging cards",
        ExchangeMode::Bump => "Exchanging cards",
        ExchangeMode::Shake => "Exchanging cards",
        _ => "Exchanging...",
    };

    ScreenModel {
        screen_id: "exchange_ble_exchanging".into(),
        title: title.into(),
        subtitle: Some("Transferring contact information securely".into()),
        components: vec![Component::Text {
            id: "ble_exchange_status".into(),
            content: "Encrypted exchange in progress...".into(),
            style: TextStyle::Body,
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

pub(super) fn build_verifying_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let subtitle = match mode {
        ExchangeMode::Magic => "Confirming proximity via audio...",
        ExchangeMode::Bump => "Confirming proximity via impact...",
        ExchangeMode::Shake => "Confirming proximity via motion...",
        _ => "Verifying proximity...",
    };

    ScreenModel {
        screen_id: "exchange_ble_verifying".into(),
        title: "Verifying".into(),
        subtitle: Some(subtitle.into()),
        components: vec![],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

// ── Action handler ─────────────────────────────────────────────────────────

pub(super) fn handle_ble_action(step: &BleStep, action: &UserAction) -> Option<BleActionOutcome> {
    match (step, action) {
        (_, UserAction::ActionPressed { action_id }) if action_id == "cancel" => {
            Some(BleActionOutcome::Cancel)
        }
        (_, UserAction::ActionPressed { action_id }) if action_id == "fallback_relay" => {
            Some(BleActionOutcome::FallbackToRelay)
        }
        _ => None,
    }
}

// ── BLE exchange flow ─────────────────────────────────────────────────────

/// BLE exchange flow state machine.
///
/// Drives discovery → connection → exchange → proximity verification
/// for Magic, Bump, and Shake modes. The parent `ExchangeEngine`
/// creates this when entering a BLE mode and routes hardware events
/// through it.
///
/// ADR-031: emits `ExchangeCommand`s via outcomes; never calls
/// hardware directly.
pub(super) struct BleExchangeFlow {
    mode: ExchangeMode,
    step: BleStep,
    connected_device: Option<String>,
    proximity_runner: Option<ProximityRunner>,
    /// Card bytes received from BLE exchange (set on handshake complete).
    received_card: Option<Vec<u8>>,
}

impl BleExchangeFlow {
    pub(super) fn new(mode: ExchangeMode) -> Self {
        Self {
            mode,
            step: BleStep::Discovering,
            connected_device: None,
            proximity_runner: None,
            received_card: None,
        }
    }

    pub(super) fn step(&self) -> &BleStep {
        &self.step
    }

    pub(super) fn mode(&self) -> ExchangeMode {
        self.mode
    }

    pub(super) fn proximity_result(&self) -> Option<&ProximityRunnerResult> {
        self.proximity_runner.as_ref()?.result()
    }

    /// Process a hardware event and return the outcome.
    pub(super) fn handle_event(&mut self, event: &ExchangeHardwareEvent) -> BleHardwareOutcome {
        // BLE disconnection is always a failure (any step)
        if let ExchangeHardwareEvent::BleDisconnected { reason } = event {
            return BleHardwareOutcome::FailedWithFallback {
                reason: format!("BLE disconnected: {reason}"),
            };
        }

        // Hardware errors/unavailability for BLE transport
        if let ExchangeHardwareEvent::HardwareError { transport, error } = event {
            if transport.eq_ignore_ascii_case("ble") {
                return BleHardwareOutcome::FailedWithFallback {
                    reason: error.clone(),
                };
            }
        }
        if let ExchangeHardwareEvent::HardwareUnavailable { transport } = event {
            if transport.eq_ignore_ascii_case("ble") {
                return BleHardwareOutcome::FailedWithFallback {
                    reason: "Bluetooth not available".into(),
                };
            }
        }

        match &self.step {
            BleStep::Discovering => self.handle_discovering(event),
            BleStep::Handshaking => self.handle_handshaking(event),
            BleStep::Exchanging => self.handle_exchanging(event),
            BleStep::Verifying => self.handle_verifying(event),
            BleStep::Complete => BleHardwareOutcome::Ignored,
        }
    }

    fn handle_discovering(&mut self, event: &ExchangeHardwareEvent) -> BleHardwareOutcome {
        if let ExchangeHardwareEvent::BleDeviceDiscovered { id, .. } = event {
            self.connected_device = Some(id.clone());
            self.step = BleStep::Handshaking;
            return BleHardwareOutcome::StepAdvanced {
                commands: vec![ExchangeCommand::BleConnect {
                    device_id: id.clone(),
                }],
            };
        }
        BleHardwareOutcome::Ignored
    }

    fn handle_handshaking(&mut self, event: &ExchangeHardwareEvent) -> BleHardwareOutcome {
        if let ExchangeHardwareEvent::BleConnected { .. } = event {
            self.step = BleStep::Exchanging;
            // Start proximity runner for this mode
            let proximity_method = proximity_method_for_mode(self.mode);
            let runner = ProximityRunner::new(proximity_method);
            let commands = runner.start();
            self.proximity_runner = Some(runner);
            return BleHardwareOutcome::StepAdvanced { commands };
        }
        BleHardwareOutcome::Ignored
    }

    fn handle_exchanging(&mut self, event: &ExchangeHardwareEvent) -> BleHardwareOutcome {
        // Feed proximity events to runner
        if is_proximity_event(event) {
            if let Some(ref mut runner) = self.proximity_runner {
                let commands = runner.feed_event(event);
                if runner.is_done() {
                    // Proximity done — if we also have card data, complete
                    if self.received_card.is_some() {
                        return self.try_complete(commands);
                    }
                    self.step = BleStep::Verifying;
                    return BleHardwareOutcome::StepAdvanced { commands };
                }
                return BleHardwareOutcome::Consumed { commands };
            }
        }

        // BLE characteristic data = card exchange
        if let ExchangeHardwareEvent::BleCharacteristicNotified { data, .. } = event {
            if !data.is_empty() {
                self.received_card = Some(data.clone());
                // If proximity is already done, complete
                if self.proximity_runner.as_ref().is_some_and(|r| r.is_done()) {
                    return self.try_complete(vec![]);
                }
                return BleHardwareOutcome::Consumed { commands: vec![] };
            }
        }

        BleHardwareOutcome::Ignored
    }

    fn handle_verifying(&mut self, event: &ExchangeHardwareEvent) -> BleHardwareOutcome {
        // In verifying, we're waiting for card data (proximity already done)
        if let ExchangeHardwareEvent::BleCharacteristicNotified { data, .. } = event {
            if !data.is_empty() {
                self.received_card = Some(data.clone());
                return self.try_complete(vec![]);
            }
        }
        BleHardwareOutcome::Ignored
    }

    /// Transition to Complete if both card data and proximity result are available.
    fn try_complete(&mut self, extra_commands: Vec<ExchangeCommand>) -> BleHardwareOutcome {
        if let Some(card_bytes) = self.received_card.take() {
            self.step = BleStep::Complete;
            let mut commands = extra_commands;
            commands.push(ExchangeCommand::AccelerometerStop);
            commands.push(ExchangeCommand::BleDisconnect);
            BleHardwareOutcome::Complete {
                card_bytes,
                commands,
            }
        } else {
            BleHardwareOutcome::Consumed {
                commands: extra_commands,
            }
        }
    }
}

/// Map exchange mode to the proximity verification method.
fn proximity_method_for_mode(mode: ExchangeMode) -> ProximityMethod {
    match mode {
        ExchangeMode::Magic => ProximityMethod::Audio,
        ExchangeMode::Bump => ProximityMethod::Impact,
        ExchangeMode::Shake => ProximityMethod::Accelerometer,
        _ => ProximityMethod::Audio,
    }
}

/// Whether the event is a proximity verification event.
fn is_proximity_event(event: &ExchangeHardwareEvent) -> bool {
    matches!(
        event,
        ExchangeHardwareEvent::AudioResponseReceived { .. }
            | ExchangeHardwareEvent::AccelerometerData { .. }
            | ExchangeHardwareEvent::ImpactDetected { .. }
    )
}

// INLINE_TEST_REQUIRED: tests access private fields and internal state transitions
#[cfg(test)]
mod tests {
    use super::*;

    // ── Discovery tests ────────────────────────────────────────────

    #[test]
    fn discovery_emits_connect_on_device_found() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        assert_eq!(*flow.step(), BleStep::Discovering);

        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![],
        });

        assert_eq!(*flow.step(), BleStep::Handshaking);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(matches!(
                    &commands[0],
                    ExchangeCommand::BleConnect { device_id } if device_id == "device-1"
                ));
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    #[test]
    fn discovery_ignores_non_ble_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&ExchangeHardwareEvent::QrScanned {
            data: "test".into(),
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
        assert_eq!(*flow.step(), BleStep::Discovering);
    }

    // ── Connection tests ───────────────────────────────────────────

    #[test]
    fn connection_starts_proximity_and_advances_to_exchanging() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        // Discover
        flow.handle_event(&ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        assert_eq!(*flow.step(), BleStep::Handshaking);

        // Connect
        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleConnected {
            device_id: "d1".into(),
        });

        assert_eq!(*flow.step(), BleStep::Exchanging);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                // Magic mode → Audio proximity → AudioEmitChallenge
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, ExchangeCommand::AudioEmitChallenge { .. }))
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    #[test]
    fn bump_connection_starts_accelerometer() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        flow.handle_event(&ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleConnected {
            device_id: "d1".into(),
        });

        assert_eq!(*flow.step(), BleStep::Exchanging);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, ExchangeCommand::AccelerometerStart))
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // ── Proximity + exchange tests ─────────────────────────────────

    #[test]
    fn impact_event_during_exchange_completes_with_card() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        // Receive card data first
        flow.handle_event(&ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });

        // Then impact → should complete
        let outcome = flow.handle_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000, // 3g > 2.5g threshold
        });

        assert_eq!(*flow.step(), BleStep::Complete);
        match outcome {
            BleHardwareOutcome::Complete {
                card_bytes,
                commands,
            } => {
                assert_eq!(card_bytes, vec![1, 2, 3]);
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, ExchangeCommand::BleDisconnect))
                );
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn proximity_done_before_card_advances_to_verifying() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        // Impact first (no card yet)
        let outcome = flow.handle_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));

        // Then card → complete
        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![4, 5, 6],
        });
        assert_eq!(*flow.step(), BleStep::Complete);
        match outcome {
            BleHardwareOutcome::Complete { card_bytes, .. } => {
                assert_eq!(card_bytes, vec![4, 5, 6]);
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn card_without_proximity_stays_in_exchanging() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });

        // Still exchanging — waiting for proximity
        assert_eq!(*flow.step(), BleStep::Exchanging);
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
    }

    // ── Failure tests ──────────────────────────────────────────────

    #[test]
    fn ble_disconnect_fails_at_any_step() {
        for mode in [ExchangeMode::Magic, ExchangeMode::Bump, ExchangeMode::Shake] {
            let mut flow = BleExchangeFlow::new(mode);
            let outcome = flow.handle_event(&ExchangeHardwareEvent::BleDisconnected {
                reason: "timeout".into(),
            });
            assert!(
                matches!(outcome, BleHardwareOutcome::FailedWithFallback { .. }),
                "Expected FailedWithFallback for {mode:?}"
            );
        }
    }

    #[test]
    fn ble_hardware_error_fails() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&ExchangeHardwareEvent::HardwareError {
            transport: "BLE".into(),
            error: "adapter off".into(),
        });
        match outcome {
            BleHardwareOutcome::FailedWithFallback { reason } => {
                assert_eq!(reason, "adapter off");
            }
            other => panic!("Expected FailedWithFallback, got {other:?}"),
        }
    }

    #[test]
    fn ble_unavailable_fails() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&ExchangeHardwareEvent::HardwareUnavailable {
            transport: "ble".into(),
        });
        assert!(matches!(
            outcome,
            BleHardwareOutcome::FailedWithFallback { .. }
        ));
    }

    #[test]
    fn non_ble_hardware_error_is_ignored() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&ExchangeHardwareEvent::HardwareError {
            transport: "NFC".into(),
            error: "not supported".into(),
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
    }

    // ── Complete step ignores events ───────────────────────────────

    #[test]
    fn complete_step_ignores_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);
        // Get card + impact to complete
        flow.handle_event(&ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "c".into(),
            data: vec![1],
        });
        flow.handle_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Complete);

        // Further events ignored
        let outcome = flow.handle_event(&ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "d2".into(),
            rssi: -50,
            adv_data: vec![],
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
    }

    // ── Mode-specific proximity method ─────────────────────────────

    #[test]
    fn magic_uses_audio_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Magic),
            ProximityMethod::Audio
        );
    }

    #[test]
    fn bump_uses_impact_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Bump),
            ProximityMethod::Impact
        );
    }

    #[test]
    fn shake_uses_accelerometer_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Shake),
            ProximityMethod::Accelerometer
        );
    }

    // ── Helper ─────────────────────────────────────────────────────

    /// Advance a flow to Exchanging step (discovery + connection).
    fn advance_to_exchanging(flow: &mut BleExchangeFlow) {
        flow.handle_event(&ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        flow.handle_event(&ExchangeHardwareEvent::BleConnected {
            device_id: "d1".into(),
        });
        assert_eq!(*flow.step(), BleStep::Exchanging);
    }
}

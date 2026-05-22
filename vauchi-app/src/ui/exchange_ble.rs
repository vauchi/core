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
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::exchange::proximity_runner::{ProximityMethod, ProximityRunner};
use vauchi_core::{Command, Event};

// ── Step enum ──────────────────────────────────────────────────────────────

/// Steps specific to the BLE exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
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
#[allow(dead_code)] // card_bytes used when card saving is integrated
pub(super) enum BleHardwareOutcome {
    /// Step advanced — parent should update screen. May emit commands.
    StepAdvanced { commands: Vec<Command> },
    /// BLE exchange completed — card bytes available.
    Complete {
        card_bytes: Vec<u8>,
        commands: Vec<Command>,
    },
    /// BLE failed — offer relay fallback.
    FailedWithFallback { reason: String },
    /// Event consumed but no step change. May emit commands.
    Consumed { commands: Vec<Command> },
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
            a11y: None,
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
/// ADR-031: emits `Command`s via outcomes; never calls
/// hardware directly.
/// BLE data characteristic UUID used for shake envelope exchange.
const SHAKE_ENVELOPE_CHAR: &str = vauchi_core::exchange::CHAR_DATA_WRITE;

pub(super) struct BleExchangeFlow {
    mode: ExchangeMode,
    step: BleStep,
    connected_device: Option<String>,
    proximity_runner: Option<ProximityRunner>,
    /// Card bytes received from BLE exchange (set on handshake complete).
    received_card: Option<Vec<u8>>,
    /// Whether our shake envelope has been sent to the peer.
    shake_envelope_sent: bool,
}

impl BleExchangeFlow {
    pub(super) fn new(mode: ExchangeMode) -> Self {
        Self {
            mode,
            step: BleStep::Discovering,
            connected_device: None,
            proximity_runner: None,
            received_card: None,
            shake_envelope_sent: false,
        }
    }

    pub(super) fn step(&self) -> &BleStep {
        &self.step
    }

    /// Process a hardware event and return the outcome.
    pub(super) fn handle_event(&mut self, event: &Event) -> BleHardwareOutcome {
        // BLE disconnection is always a failure (any step)
        if let Event::BleDisconnected { reason } = event {
            return BleHardwareOutcome::FailedWithFallback {
                reason: format!("BLE disconnected: {reason}"),
            };
        }

        // Hardware errors/unavailability for BLE transport
        if let Event::HardwareError { transport, error } = event
            && transport.eq_ignore_ascii_case("ble")
        {
            return BleHardwareOutcome::FailedWithFallback {
                reason: error.clone(),
            };
        }
        if let Event::HardwareUnavailable { transport } = event
            && transport.eq_ignore_ascii_case("ble")
        {
            return BleHardwareOutcome::FailedWithFallback {
                reason: "Bluetooth not available".into(),
            };
        }
        if let Event::PermissionDenied { transport } = event
            && transport.eq_ignore_ascii_case("ble")
        {
            return BleHardwareOutcome::FailedWithFallback {
                reason: "Bluetooth permission denied".into(),
            };
        }

        match &self.step {
            BleStep::Discovering => self.handle_discovering(event),
            BleStep::Handshaking => self.handle_handshaking(event),
            BleStep::Exchanging => self.handle_exchanging(event),
            BleStep::Verifying => self.handle_verifying(event),
            BleStep::Complete => BleHardwareOutcome::Ignored,
        }
    }

    fn handle_discovering(&mut self, event: &Event) -> BleHardwareOutcome {
        if let Event::BleDeviceDiscovered { id, .. } = event {
            self.connected_device = Some(id.clone());
            self.step = BleStep::Handshaking;
            return BleHardwareOutcome::StepAdvanced {
                commands: vec![Command::BleConnect {
                    device_id: id.clone(),
                }],
            };
        }
        BleHardwareOutcome::Ignored
    }

    fn handle_handshaking(&mut self, event: &Event) -> BleHardwareOutcome {
        if let Event::BleConnected { .. } = event {
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

    fn handle_exchanging(&mut self, event: &Event) -> BleHardwareOutcome {
        // Feed proximity events to runner
        if is_proximity_event(event)
            && let Some(ref mut runner) = self.proximity_runner
        {
            let mut commands = runner.feed_event(event);

            // Shake mode: after samples accumulate, finish recording and send envelope
            if self.mode == ExchangeMode::Shake
                && !self.shake_envelope_sent
                && !runner.is_recording_done()
                && let Some((envelope, stop_cmds)) = runner.finish_recording()
            {
                self.shake_envelope_sent = true;
                commands.extend(stop_cmds);
                commands.push(Command::BleWriteCharacteristic {
                    uuid: SHAKE_ENVELOPE_CHAR.to_string(),
                    data: envelope,
                });
            }

            if runner.is_done() && self.received_card.is_some() {
                return self.try_complete(commands);
            } else if runner.is_done() {
                self.step = BleStep::Verifying;
                return BleHardwareOutcome::StepAdvanced { commands };
            }
            return BleHardwareOutcome::Consumed { commands };
        }

        // BLE characteristic notifications — could be card data or shake envelope
        if let Event::BleCharacteristicNotified { uuid, data } = event
            && !data.is_empty()
        {
            // Shake envelope from peer (on data write characteristic)
            if self.mode == ExchangeMode::Shake && uuid == SHAKE_ENVELOPE_CHAR {
                return self.handle_shake_envelope(data);
            }

            // Card data
            self.received_card = Some(data.clone());
            if self.proximity_runner.as_ref().is_some_and(|r| r.is_done()) {
                return self.try_complete(vec![]);
            }
            return BleHardwareOutcome::Consumed { commands: vec![] };
        }

        BleHardwareOutcome::Ignored
    }

    /// Handle received shake envelope from peer.
    fn handle_shake_envelope(&mut self, data: &[u8]) -> BleHardwareOutcome {
        if let Some(ref mut runner) = self.proximity_runner {
            let commands = runner.receive_peer_envelope(data);
            if runner.is_done() {
                if self.received_card.is_some() {
                    return self.try_complete(commands);
                }
                self.step = BleStep::Verifying;
                return BleHardwareOutcome::StepAdvanced { commands };
            }
            return BleHardwareOutcome::Consumed { commands };
        }
        BleHardwareOutcome::Ignored
    }

    fn handle_verifying(&mut self, event: &Event) -> BleHardwareOutcome {
        // In verifying, we're waiting for card data (proximity already done)
        if let Event::BleCharacteristicNotified { uuid, data } = event
            && !data.is_empty()
        {
            // Shake envelope in verifying — peer envelope arrived late
            if self.mode == ExchangeMode::Shake && uuid == SHAKE_ENVELOPE_CHAR {
                return self.handle_shake_envelope(data);
            }
            self.received_card = Some(data.clone());
            return self.try_complete(vec![]);
        }
        BleHardwareOutcome::Ignored
    }

    /// Transition to Complete if both card data and proximity result are available.
    fn try_complete(&mut self, extra_commands: Vec<Command>) -> BleHardwareOutcome {
        if let Some(card_bytes) = self.received_card.take() {
            self.step = BleStep::Complete;
            let mut commands = extra_commands;
            commands.push(Command::AccelerometerStop);
            commands.push(Command::BleDisconnect);
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
fn is_proximity_event(event: &Event) -> bool {
    matches!(
        event,
        Event::AudioSamplesRecorded { .. }
            | Event::AccelerometerData { .. }
            | Event::ImpactDetected { .. }
    )
}

// INLINE_TEST_REQUIRED: tests access private fields and internal state transitions
#[cfg(test)]
mod tests {
    use super::*;

    // ── Discovery tests ────────────────────────────────────────────

    // @internal
    #[test]
    fn discovery_emits_connect_on_device_found() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        assert_eq!(*flow.step(), BleStep::Discovering);

        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
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
                    Command::BleConnect { device_id } if device_id == "device-1"
                ));
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn discovery_ignores_non_ble_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&Event::QrScanned {
            data: "test".into(),
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
        assert_eq!(*flow.step(), BleStep::Discovering);
    }

    // ── Connection tests ───────────────────────────────────────────

    // @internal
    #[test]
    fn connection_starts_proximity_and_advances_to_exchanging() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        // Discover
        flow.handle_event(&Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        assert_eq!(*flow.step(), BleStep::Handshaking);

        // Connect
        let outcome = flow.handle_event(&Event::BleConnected {
            device_id: "d1".into(),
        });

        assert_eq!(*flow.step(), BleStep::Exchanging);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                // Magic mode → Audio proximity → AudioEmitChallenge + AudioListenForResponse
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, Command::AudioEmitChallenge { .. }))
                );
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, Command::AudioListenForResponse { .. }))
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn bump_connection_starts_accelerometer() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        flow.handle_event(&Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        let outcome = flow.handle_event(&Event::BleConnected {
            device_id: "d1".into(),
        });

        assert_eq!(*flow.step(), BleStep::Exchanging);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert!(
                    commands
                        .iter()
                        .any(|c| matches!(c, Command::AccelerometerStart))
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // ── Proximity + exchange tests ─────────────────────────────────

    // @internal
    #[test]
    fn impact_event_during_exchange_completes_with_card() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        // Receive card data first
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });

        // Then impact → should complete
        let outcome = flow.handle_event(&Event::ImpactDetected {
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
                assert!(commands.iter().any(|c| matches!(c, Command::BleDisconnect)));
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn proximity_done_before_card_advances_to_verifying() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        // Impact first (no card yet)
        let outcome = flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));

        // Then card → complete
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
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

    // @internal
    #[test]
    fn card_without_proximity_stays_in_exchanging() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });

        // Still exchanging — waiting for proximity
        assert_eq!(*flow.step(), BleStep::Exchanging);
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
    }

    // ── Magic mode audio integration tests ───────────────────────

    // @internal
    #[test]
    fn magic_audio_response_completes_exchange_with_card() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        advance_to_exchanging(&mut flow);

        // Receive card data
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![10, 20, 30],
        });

        // Audio response → proximity done → exchange complete.
        // Build a real FSK-encoded sample buffer so the proximity
        // runner's decode succeeds and the verified flag flips.
        let modem_config = vauchi_core::exchange::audio_modem::AudioConfig::default();
        let samples =
            vauchi_core::exchange::audio_modem::generate_fsk_samples(&[1, 2, 3], &modem_config);
        let outcome = flow.handle_event(&Event::AudioSamplesRecorded {
            samples,
            sample_rate: modem_config.sample_rate,
        });

        assert_eq!(*flow.step(), BleStep::Complete);
        match outcome {
            BleHardwareOutcome::Complete {
                card_bytes,
                commands,
            } => {
                assert_eq!(card_bytes, vec![10, 20, 30]);
                // Should include AudioStop from runner + BleDisconnect from flow
                assert!(commands.iter().any(|c| matches!(c, Command::AudioStop)));
                assert!(commands.iter().any(|c| matches!(c, Command::BleDisconnect)));
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
        // Verify proximity result is available
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(prox.verified);
        assert!((prox.confidence - 0.85).abs() < f32::EPSILON);
    }

    // @internal
    #[test]
    fn magic_audio_timeout_does_not_block_exchange() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        advance_to_exchanging(&mut flow);

        // Simulate audio timeout (runner times out, producing failed result)
        flow.proximity_runner.as_mut().unwrap().timeout();

        // Proximity done (failed) — should advance to verifying
        // since card data isn't here yet
        assert!(flow.proximity_runner.as_ref().unwrap().result().is_some());

        // Now receive card → should complete (audio failure = lower trust, not blocked)
        // We need to process a BLE event to trigger the flow check.
        // Provide card data — flow should detect proximity is done and complete.
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![7, 8, 9],
        });

        assert_eq!(*flow.step(), BleStep::Complete);
        match outcome {
            BleHardwareOutcome::Complete { card_bytes, .. } => {
                assert_eq!(card_bytes, vec![7, 8, 9]);
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
        // Proximity was a timeout — verified = false but exchange still completed
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(!prox.verified);
        assert_eq!(prox.confidence, 0.0);
    }

    // ── Shake mode envelope exchange tests ───────────────────────

    /// Feed accelerometer samples to a Shake flow in Exchanging step.
    fn feed_accel_samples(flow: &mut BleExchangeFlow, count: usize) {
        for i in 0..count {
            flow.handle_event(&Event::AccelerometerData {
                x_milli_g: ((i as f32 * 0.1).sin() * 2000.0) as i32,
                y_milli_g: ((i as f32 * 0.1).cos() * 1500.0) as i32,
                z_milli_g: 1000,
                timestamp_ms: i as u64 * 10,
            });
        }
    }

    // @internal
    #[test]
    fn shake_sends_envelope_after_recording() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake);
        advance_to_exchanging(&mut flow);

        // Feed enough samples
        feed_accel_samples(&mut flow, 10);

        // Manually finish recording via the runner
        let runner = flow.proximity_runner.as_mut().unwrap();
        let (envelope, _) = runner.finish_recording().unwrap();

        // Verify envelope is non-empty and versioned
        assert!(!envelope.is_empty());
        assert_eq!(envelope[0], 0x01); // ENVELOPE_VERSION
    }

    // @internal
    #[test]
    fn shake_peer_envelope_completes_with_card() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake);
        advance_to_exchanging(&mut flow);

        // Record samples and finish recording
        feed_accel_samples(&mut flow, 50);
        let runner = flow.proximity_runner.as_mut().unwrap();
        let (our_envelope, _) = runner.finish_recording().unwrap();
        flow.shake_envelope_sent = true;

        // Receive card data
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![10, 20, 30],
        });

        // Receive peer's envelope (same data = perfect correlation)
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: SHAKE_ENVELOPE_CHAR.into(),
            data: our_envelope,
        });

        assert_eq!(*flow.step(), BleStep::Complete);
        match outcome {
            BleHardwareOutcome::Complete { card_bytes, .. } => {
                assert_eq!(card_bytes, vec![10, 20, 30]);
            }
            other => panic!("Expected Complete, got {other:?}"),
        }
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(prox.verified);
        assert!(prox.confidence <= 0.5); // Capped per spec
    }

    // @internal
    #[test]
    fn shake_peer_envelope_before_card_advances_to_verifying() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake);
        advance_to_exchanging(&mut flow);

        feed_accel_samples(&mut flow, 50);
        let runner = flow.proximity_runner.as_mut().unwrap();
        let (our_envelope, _) = runner.finish_recording().unwrap();
        flow.shake_envelope_sent = true;

        // Peer envelope first (no card yet)
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: SHAKE_ENVELOPE_CHAR.into(),
            data: our_envelope,
        });

        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));

        // Then card → complete
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });
        assert_eq!(*flow.step(), BleStep::Complete);
        assert!(matches!(outcome, BleHardwareOutcome::Complete { .. }));
    }

    // @internal
    #[test]
    fn shake_non_envelope_char_is_card_data() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake);
        advance_to_exchanging(&mut flow);

        // Data on a non-envelope UUID is treated as card data
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "some-other-char".into(),
            data: vec![1, 2],
        });
        assert_eq!(*flow.step(), BleStep::Exchanging); // Still exchanging
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
        assert!(flow.received_card.is_some());
    }

    // ── Failure tests ──────────────────────────────────────────────

    // @internal
    #[test]
    fn ble_disconnect_fails_at_any_step() {
        for mode in [ExchangeMode::Magic, ExchangeMode::Bump, ExchangeMode::Shake] {
            let mut flow = BleExchangeFlow::new(mode);
            let outcome = flow.handle_event(&Event::BleDisconnected {
                reason: "timeout".into(),
            });
            assert!(
                matches!(outcome, BleHardwareOutcome::FailedWithFallback { .. }),
                "Expected FailedWithFallback for {mode:?}"
            );
        }
    }

    // @internal
    #[test]
    fn ble_hardware_error_fails() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&Event::HardwareError {
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

    // @internal
    #[test]
    fn ble_unavailable_fails() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&Event::HardwareUnavailable {
            transport: "ble".into(),
        });
        assert!(matches!(
            outcome,
            BleHardwareOutcome::FailedWithFallback { .. }
        ));
    }

    // @internal
    #[test]
    fn non_ble_hardware_error_is_ignored() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic);
        let outcome = flow.handle_event(&Event::HardwareError {
            transport: "NFC".into(),
            error: "not supported".into(),
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
    }

    // ── Bump mode edge cases ────────────────────────────────────

    // @internal
    #[test]
    fn bump_weak_impact_completes_with_unverified_proximity() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        // Card data
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "c".into(),
            data: vec![1, 2],
        });

        // Weak impact (1g < 2.5g threshold) — still completes (impact doesn't block)
        let outcome = flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 1000, // 1g
        });

        assert_eq!(*flow.step(), BleStep::Complete);
        assert!(matches!(outcome, BleHardwareOutcome::Complete { .. }));
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(!prox.verified);
        assert!(prox.confidence < 0.6);
    }

    // @internal
    #[test]
    fn bump_strong_impact_has_capped_confidence() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);

        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "c".into(),
            data: vec![1],
        });

        // Very strong impact (10g) — confidence capped at 0.6 per spec
        flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 10000,
        });

        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(prox.verified);
        assert!((prox.confidence - 0.6).abs() < f32::EPSILON);
    }

    // ── Complete step ignores events ───────────────────────────────

    // @internal
    #[test]
    fn complete_step_ignores_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump);
        advance_to_exchanging(&mut flow);
        // Get card + impact to complete
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "c".into(),
            data: vec![1],
        });
        flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Complete);

        // Further events ignored
        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
            id: "d2".into(),
            rssi: -50,
            adv_data: vec![],
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
    }

    // ── Mode-specific proximity method ─────────────────────────────

    // @internal
    #[test]
    fn magic_uses_audio_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Magic),
            ProximityMethod::Audio
        );
    }

    // @internal
    #[test]
    fn bump_uses_impact_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Bump),
            ProximityMethod::Impact
        );
    }

    // @internal
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
        flow.handle_event(&Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        flow.handle_event(&Event::BleConnected {
            device_id: "d1".into(),
        });
        assert_eq!(*flow.step(), BleStep::Exchanging);
    }
}

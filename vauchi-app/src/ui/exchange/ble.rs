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

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::exchange::proximity_runner::{ProximityMethod, ProximityRunner};
use vauchi_core::{Command, Event};

use crate::orchestrator::ble_handshake_machine::{BleRole, decide_ble_role};

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

pub(super) fn build_discovering_screen(mode: ExchangeMode, locale: Locale) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    let (title_key, subtitle_key) = match mode {
        ExchangeMode::Magic => (
            "exchange.ble.searching_magic_title",
            "exchange.ble.searching_magic_subtitle",
        ),
        ExchangeMode::Bump => (
            "exchange.ble.ready_bump_title",
            "exchange.ble.ready_bump_subtitle",
        ),
        ExchangeMode::Shake => (
            "exchange.ble.ready_shake_title",
            "exchange.ble.ready_shake_subtitle",
        ),
        _ => (
            "exchange.ble.searching_default_title",
            "exchange.ble.searching_default_subtitle",
        ),
    };

    ScreenModel {
        screen_id: "exchange_ble_discovering".into(),
        title: t(title_key),
        subtitle: Some(t(subtitle_key)),
        components: vec![Component::Text {
            id: "ble_status".into(),
            content: t("exchange.ble.scanning"),
            style: TextStyle::Body,
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: t("action.cancel"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        }],
        ..Default::default()
    }
}

pub(super) fn build_exchanging_screen(mode: ExchangeMode, locale: Locale) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    let title_key = match mode {
        ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake => {
            "exchange.ble.exchanging_title"
        }
        _ => "exchange.ble.exchanging_default_title",
    };

    ScreenModel {
        screen_id: "exchange_ble_exchanging".into(),
        title: t(title_key),
        subtitle: Some(t("exchange.ble.transferring_subtitle")),
        components: vec![Component::Text {
            id: "ble_exchange_status".into(),
            content: t("exchange.ble.transferring_status"),
            style: TextStyle::Body,
        }],
        actions: vec![],
        ..Default::default()
    }
}

pub(super) fn build_verifying_screen(mode: ExchangeMode, locale: Locale) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    let subtitle_key = match mode {
        ExchangeMode::Magic => "exchange.ble.verifying_magic",
        ExchangeMode::Bump => "exchange.ble.verifying_bump",
        ExchangeMode::Shake => "exchange.ble.verifying_shake",
        _ => "exchange.ble.verifying_default",
    };

    ScreenModel {
        screen_id: "exchange_ble_verifying".into(),
        title: t("exchange.verifying.title"),
        subtitle: Some(t(subtitle_key)),
        components: vec![],
        actions: vec![],
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
    /// VESTIGIAL (P4): the encrypted card transfer is owned by the real
    /// `BleHandshakeMachine`, so this is never populated anymore. It,
    /// `try_complete`, and `BleHardwareOutcome::Complete` are kept inert
    /// to bound the P4 diff — a follow-up tidy removes them outright.
    received_card: Option<Vec<u8>>,
    /// Whether our shake envelope has been sent to the peer.
    shake_envelope_sent: bool,
    /// Last MTU reported by the GATT stack via `Event::BleMtuNegotiated`
    /// (slice 32m T2.1 seam). `None` until the first negotiation lands;
    /// the full T2.2 GATT rewire reads this to size `BleChunker::new(data,
    /// mtu - 3)` on chunked writes. Decoupled from `connected_device`
    /// so a re-negotiation mid-session overwrites cleanly without
    /// resetting connection state.
    negotiated_mtu: Option<u32>,
    /// This device's role-tiebreak token (advertised in
    /// `BleStartAdvertising.payload`). Two peers discover each other
    /// symmetrically; on discovery each compares its own token against
    /// the peer's (carried in `BleDeviceDiscovered.adv_data`) and the
    /// lexicographically smaller token initiates the connection. Moving
    /// this compare into core retires the Android `compareTokens`
    /// frontend logic (ADR-043 Humble UI).
    own_token: Vec<u8>,
}

impl BleExchangeFlow {
    pub(super) fn new(mode: ExchangeMode, own_token: Vec<u8>) -> Self {
        Self {
            mode,
            step: BleStep::Discovering,
            connected_device: None,
            proximity_runner: None,
            received_card: None,
            shake_envelope_sent: false,
            negotiated_mtu: None,
            own_token,
        }
    }

    /// Whether this device should initiate the BLE connection (become
    /// central) given the peer's advertised tiebreak token. The smaller
    /// token wins and connects; the other waits as responder
    /// (peripheral). Equal tokens — effectively impossible for distinct
    /// identities — default to responder so neither side double-connects.
    fn decides_initiator(&self, peer_token: &[u8]) -> bool {
        // Single source of truth — the AppEngine's session-role decision
        // uses the same fn, so chrome and crypto can never disagree.
        matches!(
            decide_ble_role(&self.own_token, peer_token),
            BleRole::Initiator
        )
    }

    /// Last MTU reported by the GATT stack via `Event::BleMtuNegotiated`,
    /// or `None` if no negotiation has occurred yet. T2.2 reads this to
    /// size `BleChunker`; tests assert it is updated by the event.
    #[cfg(test)]
    pub(super) fn negotiated_mtu(&self) -> Option<u32> {
        self.negotiated_mtu
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

        // Slice 32m T2.1 seam — transport-layer MTU negotiation is
        // recorded but is NOT a step transition. The full T2.2 GATT
        // rewire reads `negotiated_mtu` to size `BleChunker::new(data,
        // mtu - 3)` on chunked writes; for now we just track the
        // most recent value (re-negotiations mid-session overwrite
        // cleanly) and report the event consumed with no commands so
        // the flow stays in its current step.
        if let Event::BleMtuNegotiated { mtu, .. } = event {
            self.negotiated_mtu = Some(*mtu);
            return BleHardwareOutcome::Consumed { commands: vec![] };
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
        if let Event::BleDeviceDiscovered { id, adv_data, .. } = event {
            self.connected_device = Some(id.clone());
            self.step = BleStep::Handshaking;
            // Glance (one-sided QR) connects asymmetrically to the scanned peer
            // via the AppEngine (`handle_glance_discovery`); the symmetric
            // tiebreak here would race that and re-open the multi-peer latch (F1).
            if self.mode != ExchangeMode::Glance && self.decides_initiator(adv_data) {
                // We win the tiebreak → initiate the connection (central).
                return BleHardwareOutcome::StepAdvanced {
                    commands: vec![Command::BleConnect {
                        device_id: id.clone(),
                    }],
                };
            }
            // Responder (peripheral) → wait for the initiator to connect;
            // the `BleConnected` event drives the next step.
            return BleHardwareOutcome::StepAdvanced { commands: vec![] };
        }
        BleHardwareOutcome::Ignored
    }

    fn handle_handshaking(&mut self, event: &Event) -> BleHardwareOutcome {
        if let Event::BleConnected { .. } = event {
            self.step = BleStep::Exchanging;
            // Start a proximity runner only for modes with a proximity signal
            // (Magic/Bump/Shake). G3: Glance has none — the QR scan + BLE range
            // is the co-presence proof, so no runner; the real handshake drives
            // completion via `force_success`.
            let commands = match proximity_method_for_mode(self.mode) {
                Some(method) => {
                    let runner = ProximityRunner::new(method);
                    let commands = runner.start();
                    self.proximity_runner = Some(runner);
                    commands
                }
                None => Vec::new(),
            };
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

            // The encrypted card transfer is owned by the real
            // `BleHandshakeMachine` (P2/P3). The hollow flow no longer
            // treats notified bytes as a card — doing so raced the real
            // machine to a garbage completion that tore the session down.
            // Everything non-Shake is consumed so the flow holds its step
            // while the real handshake runs; terminal Success is driven by
            // the machine's completion via
            // `BleExchangeEngine::force_success`.
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
            // Card transfer is the real machine's job (P3); consume.
            return BleHardwareOutcome::Consumed { commands: vec![] };
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
fn proximity_method_for_mode(mode: ExchangeMode) -> Option<ProximityMethod> {
    match mode {
        ExchangeMode::Magic => Some(ProximityMethod::Audio),
        ExchangeMode::Bump => Some(ProximityMethod::Impact),
        ExchangeMode::Shake => Some(ProximityMethod::Accelerometer),
        // G3: Glance has no proximity signal — the QR scan + BLE range is the
        // co-presence proof.
        _ => None,
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
    fn discovery_initiator_with_smaller_token_emits_connect() {
        // Our token sorts before the peer's → we win the tiebreak and
        // initiate the connection (central).
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![0x01]);
        assert_eq!(*flow.step(), BleStep::Discovering);

        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0x09], // peer token > ours
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
    fn glance_discovery_never_tiebreak_connects() {
        // Glance uses scan-driven asymmetric connection (the AppEngine's
        // `handle_glance_discovery` connects to the scanned peer). The flow must
        // NOT tiebreak-connect — even when our token would win — or it races the
        // scan-driven connect and re-introduces the multi-peer latch (F1).
        let mut flow = BleExchangeFlow::new(ExchangeMode::Glance, vec![0x01]);
        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0x09], // peer token > ours → we would win the tiebreak
        });
        assert_eq!(*flow.step(), BleStep::Handshaking);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert!(
                    commands.is_empty(),
                    "Glance must not tiebreak-connect (scan-driven connect owns it), got {commands:?}"
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn discovery_responder_with_larger_token_does_not_connect() {
        // Our token sorts after the peer's → we are the responder
        // (peripheral) and must NOT connect; we wait for `BleConnected`.
        // This is the symmetric double-connect the Android
        // `compareTokens` tiebreaker existed to prevent, now owned by
        // core (ADR-043).
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![0x09]);

        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0x01], // peer token < ours
        });

        assert_eq!(*flow.step(), BleStep::Handshaking);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert!(
                    commands.is_empty(),
                    "responder must not emit BleConnect, got {commands:?}"
                );
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn discovery_equal_tokens_default_to_responder() {
        // Astronomically rare for distinct identities, but equal tokens
        // must still not double-connect: both default to responder.
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![0x05]);
        let outcome = flow.handle_event(&Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0x05],
        });
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                assert!(commands.is_empty(), "equal tokens must not connect");
            }
            other => panic!("Expected StepAdvanced, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn discovery_ignores_non_ble_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
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
    fn impact_after_notify_advances_to_verifying_without_self_completing() {
        // P4: the hollow flow no longer treats a BLE notification as the
        // card — the real `BleHandshakeMachine` owns the encrypted card
        // transfer — so it never self-completes. Proximity-done advances
        // to Verifying; terminal Success is driven by the machine via
        // `BleExchangeEngine::force_success`.
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
        advance_to_exchanging(&mut flow);

        let consumed = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });
        assert!(matches!(consumed, BleHardwareOutcome::Consumed { .. }));
        assert_eq!(*flow.step(), BleStep::Exchanging);

        let outcome = flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000, // 3g > 2.5g threshold
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(
            matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }),
            "proximity-done must advance to Verifying, not Complete, got {outcome:?}"
        );
    }

    // @internal
    #[test]
    fn proximity_done_then_notify_stays_in_verifying() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
        advance_to_exchanging(&mut flow);

        // Impact first (no card yet) → Verifying.
        let outcome = flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));

        // P4: a later notification is consumed (the real machine owns the
        // card transfer); the flow does not self-complete.
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![4, 5, 6],
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
    }

    // @internal
    #[test]
    fn card_without_proximity_stays_in_exchanging() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
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
    fn magic_audio_response_verifies_proximity_without_self_completing() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        advance_to_exchanging(&mut flow);

        // Receive card data
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![10, 20, 30],
        });

        // Audio response → proximity done → Verifying (no self-complete).
        // Build a real FSK-encoded sample buffer so the proximity
        // runner's decode succeeds and the verified flag flips.
        let modem_config = vauchi_core::exchange::audio_modem::AudioConfig::default();
        let samples =
            vauchi_core::exchange::audio_modem::generate_fsk_samples(&[1, 2, 3], &modem_config);
        let outcome = flow.handle_event(&Event::AudioSamplesRecorded {
            samples,
            sample_rate: modem_config.sample_rate,
        });

        assert_eq!(*flow.step(), BleStep::Verifying);
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands } => {
                // The runner still stops audio; the flow no longer emits a
                // completion BleDisconnect (the real machine owns that).
                assert!(commands.iter().any(|c| matches!(c, Command::AudioStop)));
            }
            other => panic!("Expected StepAdvanced to Verifying, got {other:?}"),
        }
        // The proximity runner still produced a verified result.
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(prox.verified);
        assert!((prox.confidence - 0.85).abs() < f32::EPSILON);
    }

    // @internal
    #[test]
    fn magic_audio_timeout_does_not_block_exchange() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        advance_to_exchanging(&mut flow);

        // Simulate audio timeout (runner times out, producing failed result)
        flow.proximity_runner.as_mut().unwrap().timeout();

        // Proximity done (failed) — should advance to verifying
        // since card data isn't here yet
        assert!(flow.proximity_runner.as_ref().unwrap().result().is_some());

        // P4: a later BLE notification is consumed; the flow does not
        // self-complete even after a proximity timeout (the real machine
        // owns the card transfer and completion).
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![7, 8, 9],
        });
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
        assert_ne!(*flow.step(), BleStep::Complete);

        // Proximity was a timeout — verified = false; result still readable.
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake, vec![]);
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
    fn shake_peer_envelope_verifies_proximity_without_self_completing() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake, vec![]);
        advance_to_exchanging(&mut flow);

        // Record samples and finish recording
        feed_accel_samples(&mut flow, 50);
        let runner = flow.proximity_runner.as_mut().unwrap();
        let (our_envelope, _) = runner.finish_recording().unwrap();
        flow.shake_envelope_sent = true;

        // A card-data notification is consumed (not mistaken for a card).
        let consumed = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![10, 20, 30],
        });
        assert!(matches!(consumed, BleHardwareOutcome::Consumed { .. }));

        // Receive peer's envelope (same data = perfect correlation) →
        // proximity verifies and advances to Verifying, but the flow no
        // longer self-completes (the real machine drives Success).
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: SHAKE_ENVELOPE_CHAR.into(),
            data: our_envelope,
        });

        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(prox.verified);
        assert!(prox.confidence <= 0.5); // Capped per spec
    }

    // @internal
    #[test]
    fn shake_peer_envelope_before_card_advances_to_verifying() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake, vec![]);
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

        // P4: a later card notification is consumed (the real machine
        // owns the card transfer); the flow stays in Verifying.
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "card-char".into(),
            data: vec![1, 2, 3],
        });
        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
    }

    // @internal
    #[test]
    fn shake_non_envelope_notification_is_consumed() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Shake, vec![]);
        advance_to_exchanging(&mut flow);

        // P4: a non-envelope BLE notification is consumed (the real
        // machine owns the encrypted card transfer); the hollow flow no
        // longer stores it as a card.
        let outcome = flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "some-other-char".into(),
            data: vec![1, 2],
        });
        assert_eq!(*flow.step(), BleStep::Exchanging);
        assert!(matches!(outcome, BleHardwareOutcome::Consumed { .. }));
    }

    // ── Failure tests ──────────────────────────────────────────────

    // @internal
    #[test]
    fn ble_disconnect_fails_at_any_step() {
        for mode in [ExchangeMode::Magic, ExchangeMode::Bump, ExchangeMode::Shake] {
            let mut flow = BleExchangeFlow::new(mode, vec![]);
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
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
    fn ble_permission_denied_mid_wait_fails() {
        // T2.3 mid-session race: a BLE permission revoked WHILE the engine is
        // waiting (Discovering) must fail to the same retry/cancel outcome as
        // HardwareUnavailable, not hang. Pins the existing PermissionDenied
        // branch so a refactor cannot silently re-open the forever-scan.
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        let outcome = flow.handle_event(&Event::PermissionDenied {
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
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        let outcome = flow.handle_event(&Event::HardwareError {
            transport: "NFC".into(),
            error: "not supported".into(),
        });
        assert!(matches!(outcome, BleHardwareOutcome::Ignored));
    }

    // ── Bump mode edge cases ────────────────────────────────────

    // @internal
    #[test]
    fn bump_weak_impact_advances_to_verifying_with_unverified_proximity() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
        advance_to_exchanging(&mut flow);

        // Notification consumed (not mistaken for a card).
        flow.handle_event(&Event::BleCharacteristicNotified {
            uuid: "c".into(),
            data: vec![1, 2],
        });

        // Weak impact (1g < 2.5g) resolves proximity (unverified) and
        // advances to Verifying; the flow does not self-complete.
        let outcome = flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 1000, // 1g
        });

        assert_eq!(*flow.step(), BleStep::Verifying);
        assert!(matches!(outcome, BleHardwareOutcome::StepAdvanced { .. }));
        let prox = flow.proximity_runner.as_ref().unwrap().result().unwrap();
        assert!(!prox.verified);
        assert!(prox.confidence < 0.6);
    }

    // @internal
    #[test]
    fn bump_strong_impact_has_capped_confidence() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
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
    fn verifying_step_ignores_stray_discovery_events() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
        advance_to_exchanging(&mut flow);
        // Impact resolves proximity → Verifying (the flow's terminal step;
        // real completion is the machine's job via force_success).
        flow.handle_event(&Event::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert_eq!(*flow.step(), BleStep::Verifying);

        // A stray re-discovery is ignored.
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
            Some(ProximityMethod::Audio)
        );
    }

    // @internal
    #[test]
    fn bump_uses_impact_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Bump),
            Some(ProximityMethod::Impact)
        );
    }

    // @internal
    #[test]
    fn shake_uses_accelerometer_proximity() {
        assert_eq!(
            proximity_method_for_mode(ExchangeMode::Shake),
            Some(ProximityMethod::Accelerometer)
        );

        // G3: Glance has no proximity signal (QR scan + BLE range is the proof).
        assert_eq!(proximity_method_for_mode(ExchangeMode::Glance), None);
    }

    // ── T2.1 RED — BLE MTU negotiation + subscribe-notify hypothesis ──
    //
    // Pinning the two observable contracts the slice 32m T0.2 design
    // (`_private/docs/designs/2026-05-28-slice-32m-phase-0-event-command-mapping-design.md`)
    // §3.1 + §3.2 call out for Phase 2 RED:
    //
    // - **MTU consumption**: the flow must consume
    //   `Event::BleMtuNegotiated`, not treat it as `Ignored`. Today
    //   the handshaking step's `handle_event` falls through to
    //   `Ignored` for unknown events; T2.2 GREEN wires the mtu into
    //   the flow's state so subsequent writes chunk to it. The test
    //   asserts the *response shape* (Consumed/StepAdvanced — never
    //   Ignored) rather than a specific internal field so T2.2 has
    //   freedom in how it stores the value.
    //
    // - **subscribe_notify hypothesis**: a happy-path BLE handshake
    //   must drive itself from `BleDeviceDiscovered` →
    //   `BleConnected` → `BleCharacteristicNotified` *without* the
    //   flow emitting a `Command::BleSubscribeNotify` step. Today
    //   that variant doesn't exist on `Command`, so a name-based
    //   scan of every emitted command in the trace catches a
    //   regression (any future variant added in T2.2 that fires
    //   subscribe). Confirming the hypothesis green is what unblocks
    //   T3.1's retire-`mobile_ble.rs::subscribe_notify` step.

    // @internal
    #[test]
    fn ble_mtu_negotiated_event_is_consumed_not_ignored() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Bump, vec![]);
        advance_to_exchanging(&mut flow);

        let outcome = flow.handle_event(&Event::BleMtuNegotiated {
            device_id: "d1".into(),
            mtu: 247,
        });

        // The T2.1 seam consumes the event with no commands; the
        // step must stay Exchanging because MTU negotiation isn't a
        // protocol transition.
        match outcome {
            BleHardwareOutcome::Consumed { commands } => {
                assert!(commands.is_empty(), "MTU consumption must emit no commands");
            }
            other => panic!("expected Consumed for BleMtuNegotiated, got {other:?}"),
        }
        assert_eq!(flow.negotiated_mtu(), Some(247));
        assert_eq!(*flow.step(), BleStep::Exchanging);
    }

    // @internal
    #[test]
    fn ble_mtu_negotiated_during_handshaking_is_consumed_not_ignored() {
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        // Step into Handshaking via BleDeviceDiscovered, but stop
        // before BleConnected. MTU often arrives between connection
        // and subscription on Android (post-connect `requestMtu`).
        flow.handle_event(&Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -50,
            adv_data: vec![],
        });
        assert_eq!(*flow.step(), BleStep::Handshaking);

        let outcome = flow.handle_event(&Event::BleMtuNegotiated {
            device_id: "d1".into(),
            mtu: 247,
        });

        match outcome {
            BleHardwareOutcome::Consumed { commands } => {
                assert!(commands.is_empty(), "MTU consumption must emit no commands");
            }
            other => panic!("expected Consumed for BleMtuNegotiated, got {other:?}"),
        }
        assert_eq!(flow.negotiated_mtu(), Some(247));
        // Step must NOT regress / advance — MTU arrival is a
        // transport-layer signal, not a step transition.
        assert_eq!(*flow.step(), BleStep::Handshaking);

        // Re-negotiation mid-session must overwrite cleanly.
        let _ = flow.handle_event(&Event::BleMtuNegotiated {
            device_id: "d1".into(),
            mtu: 517,
        });
        assert_eq!(flow.negotiated_mtu(), Some(517));
    }

    // @internal
    #[test]
    fn happy_path_emits_no_subscribe_notify_command() {
        // Verification of the favored hypothesis (T0.2 design §3.1):
        // frontends auto-subscribe on connect, so the flow never
        // needs to emit a subscribe_notify Command. If T2.2 GREEN
        // (or a future change) adds a Command variant whose name
        // contains "SubscribeNotify", this test fails and the
        // hypothesis is invalidated — `mobile_ble.rs::subscribe_notify`
        // would then need a Command variant before T3.1 can retire
        // the delegate trait. Today this passes trivially because no
        // such variant exists; it pins the contract going into T2.2.
        let mut flow = BleExchangeFlow::new(ExchangeMode::Magic, vec![]);
        let mut emitted: Vec<Command> = Vec::new();

        let push = |out: BleHardwareOutcome, sink: &mut Vec<Command>| match out {
            BleHardwareOutcome::StepAdvanced { commands }
            | BleHardwareOutcome::Consumed { commands } => sink.extend(commands),
            _ => {}
        };

        push(
            flow.handle_event(&Event::BleDeviceDiscovered {
                id: "d1".into(),
                rssi: -40,
                adv_data: vec![],
            }),
            &mut emitted,
        );
        push(
            flow.handle_event(&Event::BleConnected {
                device_id: "d1".into(),
            }),
            &mut emitted,
        );
        push(
            flow.handle_event(&Event::BleCharacteristicNotified {
                uuid: vauchi_core::exchange::CHAR_DATA_NOTIFY.into(),
                data: vec![0xCC; 32],
            }),
            &mut emitted,
        );

        for cmd in &emitted {
            let name = cmd.variant_name();
            assert!(
                !name.contains("SubscribeNotify"),
                "happy-path emitted a SubscribeNotify-shaped command ({name}) \u{2014} \
                 hypothesis from T0.2 §3.1 invalidated; T3.1 cannot retire \
                 mobile_ble::subscribe_notify without a Command variant",
            );
        }
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

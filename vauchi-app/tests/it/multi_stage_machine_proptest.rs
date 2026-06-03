// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 stateful proptest for the multi-stage exchange machine
//! (slice 32m T1.1 RED).
//!
//! Pattern source:
//! `core/vauchi-app/tests/it/exchange_ble_invariants_proptest.rs`
//! — same "feed random Events, assert cross-sequence invariants"
//! shape as the BLE branch. This file pins the multi-stage
//! invariants that T1.2 GREEN must satisfy:
//!
//! - **I1 Phase reachability.** Any sufficiently long event sequence
//!   that includes at least one `QrScanned` must drive the machine
//!   past `Preparing` (T1.1 stub fails this — it never advances).
//! - **I1b QR frame emission.** Repeated `advance` calls past the
//!   first `display_duration_ms` tick emit at least one
//!   `QrFrameReady` event. T1.1 stub fails — `advance` always
//!   returns `None`.
//! - **I2 Terminal absorption.** Once the machine enters `Completed`,
//!   `Failed`, or `Cancelled`, no subsequent event or `advance` can
//!   move it back to a non-terminal phase.
//! - **I3 Finalized-precedes-Completed.** `Completed` is only
//!   reachable through `Finalized` — there is no shortcut.
//! - **I4 No QR after terminal.** No `Command::QrDisplay` is emitted
//!   while the phase is `Completed` / `Failed` / `Cancelled`.
//! - **I5 Cancel is idempotent + absorbing.** Repeated `cancel()`
//!   calls always return `MultiStageEvent::None` and the phase stays
//!   `Cancelled`.
//! - **I6 Mode stability.** `mode()` never changes after construction.
//!
//! The four `apply_multi_stage_*_for_test` fixtures
//! (`core/vauchi-platform/src/platform_app_engine_test_helpers.rs:69–85`)
//! get retired in T1.3 once T1.2 makes this proptest green — every
//! state they push externally must be reachable via a real
//! `Event` sequence instead.

use proptest::prelude::*;
use vauchi_app::orchestrator::multi_stage_machine::{
    MultiStageEvent, MultiStageMachine, MultiStageMode, MultiStagePhase, event_to_commands,
};
use vauchi_core::exchange::{AudioConfig, AudioProximityState, audio_modem};
use vauchi_core::{Command, Event};

/// Strategy: arbitrary non-fatal events. Used by the I1 reachability
/// test so the post-`Preparing` assertion is not vacuously skipped
/// by an early fatal event (the broad `arb_event` strategy hits a
/// fatal branch with very high probability over 20+ draws).
fn arb_non_fatal_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        any::<u8>().prop_map(|n| Event::QrScanned {
            data: format!("vauchi://multistage/peer/{n:02x}"),
        }),
        (any::<bool>(), any::<Option<u8>>(), any::<bool>()).prop_map(
            |(detected, confidence, frame_skipped)| Event::QrScanProgress {
                detected,
                confidence,
                frame_skipped,
            }
        ),
        any::<u32>().prop_map(|n| Event::AudioSamplesRecorded {
            samples: vec![(n as f32).sin(); 16],
            sample_rate: 44_100,
        }),
        // Non-fatal `HardwareUnavailable` variants — best-effort
        // presentation hints. The machine must tolerate them.
        prop_oneof![
            Just("screen_brightness".to_string()),
            Just("idle_timer".to_string()),
            Just("orientation_lock".to_string()),
        ]
        .prop_map(|transport| Event::HardwareUnavailable { transport }),
        // Stray events — also non-fatal.
        Just(Event::BleDisconnected {
            reason: "spurious".into()
        }),
        Just(Event::LinkShared),
    ]
}

/// Strategy: arbitrary events the multi-stage screen plausibly
/// receives. Spans the happy ingress (QrScanned, QrScanProgress,
/// AudioSamplesRecorded), the failure modes (HardwareError,
/// HardwareUnavailable, PermissionDenied), and a few stray /
/// out-of-flow events to confirm the machine tolerates them. Used
/// for the absorption / no-regression invariants where a fatal
/// event mid-sequence is the *interesting* case.
fn arb_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        any::<u8>().prop_map(|n| Event::QrScanned {
            data: format!("vauchi://multistage/peer/{n:02x}"),
        }),
        (any::<bool>(), any::<Option<u8>>(), any::<bool>()).prop_map(
            |(detected, confidence, frame_skipped)| Event::QrScanProgress {
                detected,
                confidence,
                frame_skipped,
            }
        ),
        any::<u32>().prop_map(|n| Event::AudioSamplesRecorded {
            samples: vec![(n as f32).sin(); 16],
            sample_rate: 44_100,
        }),
        (
            prop_oneof![
                Just("camera".to_string()),
                Just("microphone".to_string()),
                Just("screen_brightness".to_string()),
                Just("orientation_lock".to_string()),
            ],
            "[a-z ]{1,16}",
        )
            .prop_map(|(transport, error)| Event::HardwareError { transport, error }),
        prop_oneof![
            Just("camera".to_string()),
            Just("microphone".to_string()),
            Just("screen_brightness".to_string()),
            Just("idle_timer".to_string()),
            Just("orientation_lock".to_string()),
        ]
        .prop_map(|transport| Event::HardwareUnavailable { transport }),
        prop_oneof![Just("camera".to_string()), Just("microphone".to_string()),]
            .prop_map(|transport| Event::PermissionDenied { transport }),
        Just(Event::BleDisconnected {
            reason: "spurious".into()
        }),
        Just(Event::NfcDataReceived {
            data: vec![0xCC, 0xDD]
        }),
        Just(Event::LinkShared),
    ]
}

/// Tiny opaque card payload for the proptest. Production code never
/// observes this — it's parsed by [`MultiStageSession`] internally.
fn fixture_local_card() -> Vec<u8> {
    (0u8..64).collect()
}

proptest! {
    /// **I1:** A long non-fatal event sequence with a guaranteed
    /// `QrScanned` first event must drive the machine past
    /// `Preparing`. Fails today — `advance` returns `None` and
    /// `handle_hardware_event` is a no-op for `QrScanned` in the
    /// T1.1 stub. GREEN when the protocol driver lands in T1.2.
    // @internal
    #[test]
    fn machine_reaches_post_preparing_phase_after_qr_scans(
        events in proptest::collection::vec(arb_non_fatal_event(), 20..40),
        tick_ms in 50u64..400,
    ) {
        // Guarantee at least one QrScanned in the trace — the
        // strategy is heterogeneous so we can't rely on a draw
        // including one. Synthesize it as the first event.
        let mut all_events = vec![Event::QrScanned {
            data: "vauchi://multistage/peer/00".into(),
        }];
        all_events.extend(events);
        let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
        let mut now: u64 = 0;
        for event in &all_events {
            now += tick_ms;
            let _ = machine.handle_hardware_event(event, now);
            let _ = machine.advance(now);
        }
        prop_assert!(
            !matches!(machine.phase(), MultiStagePhase::Preparing),
            "machine must advance past Preparing after QrScanned ingress; phase was {:?}",
            machine.phase(),
        );
    }

    /// **I1b:** Repeated `advance` calls (no frontend events at all)
    /// must emit at least one `MultiStageEvent::QrFrameReady` over
    /// many ticks — the cycle thread's per-frame display-duration
    /// loop made progress without any external input, and the new
    /// machine must too. Fails today — `advance` always returns
    /// `None` in the T1.1 stub. GREEN when T1.2 wires the per-frame
    /// `display_duration_ms` tick.
    // @internal
    #[test]
    fn machine_emits_qr_frame_under_advance_only(
        ticks in 10usize..40,
        tick_ms in 50u64..400,
    ) {
        let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
        let mut now: u64 = 0;
        let mut saw_qr_frame = false;
        for _ in 0..ticks {
            now += tick_ms;
            let advance_event = machine.advance(now);
            if matches!(advance_event, MultiStageEvent::QrFrameReady(_)) {
                saw_qr_frame = true;
            }
        }
        prop_assert!(
            saw_qr_frame,
            "machine must emit at least one QrFrameReady over {ticks} advance ticks",
        );
    }

    /// **I2:** terminal phases are absorbing — once `Failed` /
    /// `Cancelled` / `Completed`, no event or advance can move the
    /// machine back to a non-terminal phase. Passes today under the
    /// T1.1 stub (the failure-handling branches already enforce
    /// this) — pins the invariant going into T1.2.
    // @internal
    #[test]
    fn terminal_phases_are_absorbing(
        events in proptest::collection::vec(arb_event(), 0..15),
        post_events in proptest::collection::vec(arb_event(), 0..15),
        tick_ms in 50u64..400,
    ) {
        let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
        let mut now: u64 = 0;
        for event in &events {
            now += tick_ms;
            let _ = machine.handle_hardware_event(event, now);
            let _ = machine.advance(now);
        }
        if !machine.is_terminal() {
            return Ok(());
        }
        let terminal_phase = machine.phase();
        for event in &post_events {
            now += tick_ms;
            let post_event = machine.handle_hardware_event(event, now);
            prop_assert!(
                matches!(post_event, MultiStageEvent::None),
                "terminal machine must emit None on further events; got {post_event:?}",
            );
            let post_advance = machine.advance(now);
            prop_assert!(
                matches!(post_advance, MultiStageEvent::None),
                "terminal machine must emit None on further advances; got {post_advance:?}",
            );
            prop_assert_eq!(
                machine.phase(), terminal_phase.clone(),
                "terminal phase must not change under further events",
            );
        }
    }

    /// **I4:** no `Command::QrDisplay` emitted in or after terminal
    /// phases. Today the stub never emits any `QrDisplay` at all so
    /// this passes vacuously — keeps the invariant pinned going
    /// into T1.2, where it gates the GREEN's per-frame emission to
    /// the pre-terminal phases only.
    // @internal
    #[test]
    fn no_qr_display_in_terminal_phase(
        events in proptest::collection::vec(arb_event(), 0..20),
        tick_ms in 50u64..400,
    ) {
        let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
        let mut now: u64 = 0;
        for event in &events {
            now += tick_ms;
            let was_terminal = machine.is_terminal();
            let m_event = machine.handle_hardware_event(event, now);
            let advance_event = machine.advance(now);
            if was_terminal || machine.is_terminal() {
                for cmd in event_to_commands(&m_event)
                    .iter()
                    .chain(event_to_commands(&advance_event).iter())
                {
                    prop_assert!(
                        !matches!(cmd, Command::QrDisplay { .. }),
                        "QrDisplay must not be emitted when phase is/was terminal: {cmd:?}",
                    );
                }
            }
        }
    }

    /// **I6:** mode is stable across the machine's lifetime —
    /// arbitrary event drives must not change it.
    // @internal
    #[test]
    fn mode_is_stable(
        events in proptest::collection::vec(arb_event(), 0..20),
        tick_ms in 50u64..400,
    ) {
        let mut glance = MultiStageMachine::new_glance(fixture_local_card(), 0);
        let mut hover = MultiStageMachine::new_hover(fixture_local_card(), 0);
        let mut now: u64 = 0;
        for event in &events {
            now += tick_ms;
            let _ = glance.handle_hardware_event(event, now);
            let _ = glance.advance(now);
            let _ = hover.handle_hardware_event(event, now);
            let _ = hover.advance(now);
        }
        prop_assert_eq!(glance.mode(), MultiStageMode::Glance);
        prop_assert_eq!(hover.mode(), MultiStageMode::Hover);
    }
}

// ── Unit tests — sanity assertions that don't need proptest ────────

/// **I3 explicit:** a freshly constructed machine cannot be
/// `Completed`. T1.2 strengthens this into a Finalized-precedes-
/// Completed trace assertion once the protocol driver lands.
// @internal
#[test]
fn finalized_phase_strictly_precedes_completed_in_constructor_walk() {
    let machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
    assert!(!matches!(machine.phase(), MultiStagePhase::Completed));
}

/// **I5:** repeated `cancel` returns `None` and the phase stays
/// `Cancelled`. Further events on a cancelled machine are inert.
// @internal
#[test]
fn cancel_is_idempotent_and_absorbing() {
    let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
    let first = machine.cancel();
    let second = machine.cancel();
    assert!(matches!(first, MultiStageEvent::None));
    assert!(matches!(second, MultiStageEvent::None));
    assert!(matches!(machine.phase(), MultiStagePhase::Cancelled));
    let post = machine.handle_hardware_event(
        &Event::QrScanned {
            data: "vauchi://post-cancel".into(),
        },
        100,
    );
    assert!(matches!(post, MultiStageEvent::None));
    assert!(matches!(machine.phase(), MultiStagePhase::Cancelled));
}

/// **I2 cross-check:** `Failed` is absorbing under `advance`.
// @internal
#[test]
fn failed_phase_is_absorbing_under_advance() {
    let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
    let _ = machine.handle_hardware_event(
        &Event::PermissionDenied {
            transport: "camera".into(),
        },
        0,
    );
    assert!(matches!(machine.phase(), MultiStagePhase::Failed { .. }));
    let advance = machine.advance(100);
    assert!(matches!(advance, MultiStageEvent::None));
    assert!(matches!(machine.phase(), MultiStagePhase::Failed { .. }));
}

/// **T1.2c-audio:** Glance machines never emit audio commands —
/// `try_audio_handshake_start` is a no-op for the mode gate.
// @internal
#[test]
fn glance_machine_emits_no_audio_commands() {
    let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
    let cmds = machine.try_audio_handshake_start();
    assert!(
        cmds.is_empty(),
        "Glance must not emit audio commands: {cmds:?}",
    );
    assert_eq!(machine.mode(), MultiStageMode::Glance);
}

/// **T1.2c-audio:** Hover machines emit the paired
/// `AudioEmitChallenge` + `AudioListenForResponse` commands on
/// `try_audio_handshake_start`, and decline a second call
/// (idempotent — the inner state has moved past `Pending`).
// @internal
#[test]
fn hover_machine_emits_paired_audio_commands_once() {
    let mut machine = MultiStageMachine::new_hover(fixture_local_card(), 0);
    let cmds = machine.try_audio_handshake_start();
    assert_eq!(cmds.len(), 2, "Hover must emit exactly 2 audio commands");
    assert!(matches!(cmds[0], Command::AudioEmitChallenge { .. }));
    assert!(matches!(cmds[1], Command::AudioListenForResponse { .. }));
    // Second call is a no-op — audio_proximity moved to Listening.
    let repeat = machine.try_audio_handshake_start();
    assert!(
        repeat.is_empty(),
        "second handshake call must be a no-op: {repeat:?}",
    );
}

/// **T1.2c-audio:** `Event::AudioSamplesRecorded` on a Glance
/// machine is inert (mode gate). Returns `None`, never produces
/// an `AudioProximityChanged` event.
// @internal
#[test]
fn glance_machine_ignores_audio_samples() {
    let mut machine = MultiStageMachine::new_glance(fixture_local_card(), 0);
    let event = machine.handle_hardware_event(
        &Event::AudioSamplesRecorded {
            samples: vec![0.1; 32],
            sample_rate: 44_100,
        },
        0,
    );
    assert!(matches!(event, MultiStageEvent::None));
}

/// **T1.2c-audio:** A Hover machine that hasn't started the
/// handshake (still `Pending`) ignores incoming audio samples —
/// the state gate prevents `Pending` -> `Confirmed`/`Failed`
/// transitions that would skip `Listening`.
// @internal
#[test]
fn hover_machine_ignores_audio_samples_before_handshake_starts() {
    let mut machine = MultiStageMachine::new_hover(fixture_local_card(), 0);
    let event = machine.handle_hardware_event(
        &Event::AudioSamplesRecorded {
            samples: vec![0.1; 32],
            sample_rate: 44_100,
        },
        0,
    );
    assert!(matches!(event, MultiStageEvent::None));
}

/// **T1.2c-audio:** Malformed audio samples after the handshake
/// has started transition Hover's audio proximity to `Failed`
/// (Listening -> Failed). The matching `MultiStageEvent` carries
/// that state.
// @internal
#[test]
fn hover_machine_transitions_to_failed_on_malformed_audio() {
    let mut machine = MultiStageMachine::new_hover(fixture_local_card(), 0);
    let _ = machine.try_audio_handshake_start();
    // 32 zero samples decode to nothing — preamble not found.
    let event = machine.handle_hardware_event(
        &Event::AudioSamplesRecorded {
            samples: vec![0.0; 32],
            sample_rate: 44_100,
        },
        100,
    );
    match event {
        MultiStageEvent::AudioProximityChanged(state) => {
            assert_eq!(state, AudioProximityState::Failed);
        }
        other => panic!("expected AudioProximityChanged(Failed); got {other:?}"),
    }
}

/// **P2.C:** TapHoverShake un-gates the audio handshake — it runs both
/// audio (Hover's signal) and the accelerometer shake in parallel, so it
/// must emit the paired audio commands exactly like Hover.
// @internal
#[test]
fn tap_hover_shake_machine_emits_paired_audio_commands_once() {
    let mut machine = MultiStageMachine::new_tap_hover_shake(fixture_local_card(), 0);
    assert_eq!(machine.mode(), MultiStageMode::TapHoverShake);
    let cmds = machine.try_audio_handshake_start();
    assert_eq!(
        cmds.len(),
        2,
        "TapHoverShake must emit exactly 2 audio commands"
    );
    assert!(matches!(cmds[0], Command::AudioEmitChallenge { .. }));
    assert!(matches!(cmds[1], Command::AudioListenForResponse { .. }));
    let repeat = machine.try_audio_handshake_start();
    assert!(
        repeat.is_empty(),
        "second handshake call must be a no-op: {repeat:?}",
    );
}

/// **P2.C:** TapHoverShake processes audio samples like Hover — malformed
/// samples after the handshake transition audio proximity to Failed via
/// the un-gated `process_audio_samples` path.
// @internal
#[test]
fn tap_hover_shake_machine_transitions_to_failed_on_malformed_audio() {
    let mut machine = MultiStageMachine::new_tap_hover_shake(fixture_local_card(), 0);
    let _ = machine.try_audio_handshake_start();
    let event = machine.handle_hardware_event(
        &Event::AudioSamplesRecorded {
            samples: vec![0.0; 32],
            sample_rate: 44_100,
        },
        100,
    );
    match event {
        MultiStageEvent::AudioProximityChanged(state) => {
            assert_eq!(state, AudioProximityState::Failed);
        }
        other => panic!("expected AudioProximityChanged(Failed); got {other:?}"),
    }
}

/// **T1.2c-audio:** `event_to_commands` for `AudioProximityChanged`
/// is empty (the engine surfaces the state via
/// `apply_multi_stage_audio_proximity`, not a Command).
// @internal
#[test]
fn event_to_commands_audio_proximity_emits_no_commands() {
    let cmds = event_to_commands(&MultiStageEvent::AudioProximityChanged(
        AudioProximityState::Confirmed,
    ));
    assert!(cmds.is_empty());
    // Sanity: the audio config matches what the FSK samples were
    // generated with — proves the audio_modem import works for tests.
    let _ = AudioConfig::default();
    let _ = audio_modem::generate_fsk_samples(&[0u8; 16], &AudioConfig::default());
}

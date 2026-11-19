// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hover driven through the *shell-facing* seam only
//! (`2026-08-18-hover-decodes-the-peer-qr-but-never-advances`).
//!
//! Every other multi-stage test reaches past that seam by calling
//! `forward_multi_stage_hardware_event` directly, so all of them passed
//! while a real Pixel 3a decoded an iPhone's `INI2` frame 1146 times
//! without the exchange ever advancing. A shell cannot call that method:
//! it renders `Command::ReplaceSurface` and reports
//! `Event::ValueChanged` on the capture node's opaque binding (ADR-021 /
//! ADR-066 — no domain vocabulary crosses the boundary). These tests use
//! that route and nothing else.

#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::{Duration, SystemTime};
use vauchi_app::orchestrator::multi_stage_machine::MultiStagePhase;
use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::platform::{
    BindingId, Command, InputValue, PresentationNode, PresentationQrPurpose, SurfaceId, SurfaceSpec,
};
use vauchi_core::{Event, api::Vauchi};

/// A device sitting on Hover with a clock the test can step.
fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    engine
}

/// The last surface the engine told the shell to render.
fn latest_surface(commands: &[Command]) -> Option<SurfaceSpec> {
    commands.iter().rev().find_map(|c| match c {
        Command::ReplaceSurface { surface } => Some(surface.clone()),
        _ => None,
    })
}

/// Hover nests the capture node inside the preview `Group`, so a flat
/// scan of `surface.nodes` would miss it.
fn qr_nodes(surface: &SurfaceSpec) -> Vec<(BindingId, PresentationQrPurpose, Vec<String>)> {
    fn walk(
        nodes: &[PresentationNode],
        out: &mut Vec<(BindingId, PresentationQrPurpose, Vec<String>)>,
    ) {
        for node in nodes {
            match node {
                PresentationNode::Qr {
                    id,
                    purpose,
                    payloads,
                    ..
                } => out.push((id.clone(), *purpose, payloads.clone())),
                PresentationNode::Group { children, .. } => walk(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&surface.nodes, &mut out);
    out
}

/// What the peer's camera would see: the payload of the display QR.
fn displayed_qr(surface: &SurfaceSpec) -> Option<String> {
    qr_nodes(surface)
        .into_iter()
        .find(|(_, purpose, _)| matches!(purpose, PresentationQrPurpose::Display))
        .and_then(|(_, _, payloads)| payloads.first().cloned())
}

/// Where a shell reports a decode: the capture QR's binding.
fn capture_binding(surface: &SurfaceSpec) -> Option<BindingId> {
    qr_nodes(surface)
        .into_iter()
        .find(|(_, purpose, _)| matches!(purpose, PresentationQrPurpose::Capture))
        .map(|(id, _, _)| id)
}

/// Report one decoded frame exactly as a shell does — an opaque text
/// value on the capture node's binding. No domain event, no direct
/// machine call. Returns the command batch the shell would render.
fn report_decode(
    engine: &mut AppEngine,
    surface_id: &SurfaceId,
    binding: &BindingId,
    qr: String,
) -> Vec<Command> {
    engine
        .dispatch(Event::ValueChanged {
            surface_id: surface_id.clone(),
            binding_id: binding.clone(),
            value: InputValue::Text(qr),
        })
        .unwrap_or_default()
}

fn is_celebrate(cmd: &Command) -> bool {
    matches!(cmd, Command::Celebrate { .. })
}

fn audio_commands(commands: &[Command]) -> Vec<&'static str> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::AudioEmitChallenge { .. } => Some("emit"),
            Command::AudioListenForResponse { .. } => Some("listen"),
            _ => None,
        })
        .collect()
}

/// One phone of a pair on an exchange screen, with what its peer shows first.
struct Voice {
    engine: AppEngine,
    surface: SurfaceSpec,
    peers_opening_frame: String,
    peers_session_id: [u8; 16],
}

/// Two phones on the same exchange screen, in the order of the audio check
/// (vauchi/private#1300): the one with the lower session id sounds first,
/// the other only listens until it has heard it.
fn the_two_voices(on_exchange: fn(&str, Arc<dyn Clock>) -> AppEngine) -> (Voice, Voice) {
    use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_qr};

    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut phones = Vec::new();
    for name in ["Alice", "Bob"] {
        let mut engine = on_exchange(name, clock.clone());
        engine.poll_notifications();
        let surface = latest_surface(&engine.initial_commands().expect("the phone renders"))
            .expect("surface");
        let opening_frame = displayed_qr(&surface).expect("an opening frame");
        let session_id = match parse_qr(&opening_frame).expect("a valid opening frame") {
            StageQr::Init { session_id, .. } => session_id,
            other => panic!("expected an opening frame, got {other:?}"),
        };
        phones.push((engine, surface, opening_frame, session_id));
    }
    phones.sort_by_key(|(_, _, _, session_id)| *session_id);
    let (second_engine, second_surface, second_frame, second_id) = phones.pop().expect("two");
    let (first_engine, first_surface, first_frame, first_id) = phones.pop().expect("two");
    (
        Voice {
            engine: first_engine,
            surface: first_surface,
            peers_opening_frame: second_frame,
            peers_session_id: second_id,
        },
        Voice {
            engine: second_engine,
            surface: second_surface,
            peers_opening_frame: first_frame,
            peers_session_id: first_id,
        },
    )
}

/// The phone reads its peer's opening frame through the capture binding.
fn read_the_peers_opening_frame(voice: &mut Voice) -> Vec<Command> {
    let binding = capture_binding(&voice.surface).expect("capture binding");
    let frame = voice.peers_opening_frame.clone();
    let commands = report_decode(
        &mut voice.engine,
        &voice.surface.surface_id,
        &binding,
        frame,
    );
    if let Some(current) = latest_surface(&commands) {
        voice.surface = current;
    }
    commands
}

// @internal — reading the peer through the capture binding starts the audio
// check for the phone whose turn it is, and only once.
#[test]
fn the_second_voice_starts_listening_once_and_the_first_waits_for_data() {
    let (mut first, mut second) = the_two_voices(engine_on_hover);

    let second_starts = read_the_peers_opening_frame(&mut second);
    let second_again = read_the_peers_opening_frame(&mut second);
    let first_starts = read_the_peers_opening_frame(&mut first);

    assert_eq!(audio_commands(&second_starts), vec!["listen"]);
    assert_eq!(audio_commands(&second_again), Vec::<&str>::new());
    assert_eq!(audio_commands(&first_starts), Vec::<&str>::new());
}

/// A device on the exchange screen of a mode the picker does not offer.
fn engine_on_tap_hover_shake(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: vauchi_core::exchange::mode::ExchangeMode::TapHoverShake,
    });
    engine
}

const AUDIO_FAILURE_TITLE: &str = "Couldn't confirm devices are close";

// @internal — a malformed recording reaches the held machine through public
// dispatch. It ends the exchange where the audio check decides, and changes
// nothing on Hover (vauchi/private#1300).
#[test]
fn a_malformed_recording_ends_the_exchange_only_where_the_audio_check_decides() {
    let shown_only_after_the_last_window = (false, true);
    let never_shown = (false, false);

    let where_audio_decides = after_six_of_each_malformed_recording(engine_on_tap_hover_shake);
    let on_hover = after_six_of_each_malformed_recording(engine_on_hover);

    assert_eq!(
        where_audio_decides,
        vec![shown_only_after_the_last_window; 7]
    );
    assert_eq!(on_hover, vec![never_shown; 7]);
}

/// The second voice listens for six windows before its check fails. For
/// each malformed recording, handed to it six times: whether the audio
/// failure is on screen after the fifth, and after the sixth.
fn after_six_of_each_malformed_recording(
    on_exchange: fn(&str, Arc<dyn Clock>) -> AppEngine,
) -> Vec<(bool, bool)> {
    let failure_shown = |engine: &AppEngine| {
        engine.current_screen().components.iter().any(|component| {
            matches!(component, Component::StatusIndicator { title, .. } if title == AUDIO_FAILURE_TITLE)
        })
    };
    let mut seen = Vec::new();
    for (samples, sample_rate) in [
        (vec![0.0; 32], 44_100),
        (vec![0.0; 1], 0),
        (vec![0.0; 32], 1),
        (vec![0.0; 32], u32::MAX),
        (vec![f32::NAN; 32], 44_100),
        (vec![f32::INFINITY; 32], 48_000),
        (vec![0.0; 352_801], 44_100),
    ] {
        let (_, mut second) = the_two_voices(on_exchange);
        let commands = read_the_peers_opening_frame(&mut second);
        assert_eq!(audio_commands(&commands), vec!["listen"]);

        for _ in 0..5 {
            second
                .engine
                .dispatch(Event::AudioSamplesRecorded {
                    samples: samples.clone(),
                    sample_rate,
                })
                .expect("hardware dispatch succeeds");
        }
        let after_the_fifth = failure_shown(&second.engine);
        second
            .engine
            .dispatch(Event::AudioSamplesRecorded {
                samples,
                sample_rate,
            })
            .expect("hardware dispatch succeeds");
        seen.push((after_the_fifth, failure_shown(&second.engine)));
    }
    seen
}

// @internal — the first voice's tone, at either native rate, confirms the
// second voice, which then answers once; a later recording changes nothing.
#[test]
fn the_second_voice_hears_the_first_at_native_sample_rates_and_answers_once() {
    use vauchi_core::exchange::audio_modem::{AudioConfig, generate_fsk_samples};

    for sample_rate in [44_100, 48_000] {
        let (_, mut second) = the_two_voices(engine_on_hover);
        let commands = read_the_peers_opening_frame(&mut second);
        assert_eq!(audio_commands(&commands), vec!["listen"]);
        let config = AudioConfig {
            sample_rate,
            ..AudioConfig::default()
        };
        let mut recording = vec![0.0; sample_rate as usize / 2];
        recording.extend(generate_fsk_samples(&second.peers_session_id, &config));
        recording.extend(vec![0.0; sample_rate as usize / 2]);

        let mut after_the_tone = second
            .engine
            .dispatch(Event::AudioSamplesRecorded {
                samples: recording,
                sample_rate,
            })
            .expect("valid audio dispatch");
        after_the_tone.extend(second.engine.drain_pending_commands());
        let mut after_noise = second
            .engine
            .dispatch(Event::AudioSamplesRecorded {
                samples: vec![0.0; 32],
                sample_rate,
            })
            .expect("late audio dispatch");
        after_noise.extend(second.engine.drain_pending_commands());

        assert_eq!(audio_commands(&after_the_tone), vec!["emit"]);
        assert_eq!(audio_commands(&after_noise), Vec::<&str>::new());
        assert_eq!(
            second.engine.current_screen().screen_id,
            "multi_stage_exchange"
        );
    }
}

// @scenario: exchange :: a reported decode advances Hover past advertising
#[test]
fn a_reported_decode_advances_hover_past_advertising() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", clock_a);
    let mut bob = engine_on_hover("Bob", clock_b);

    alice.poll_notifications();
    bob.poll_notifications();

    let a_surface = latest_surface(&alice.initial_commands().expect("alice commands"))
        .expect("Alice renders a surface");
    let b_surface =
        latest_surface(&bob.initial_commands().expect("bob commands")).expect("Bob renders");

    let bob_qr = displayed_qr(&b_surface).expect("Bob displays an INIT frame");
    let binding = capture_binding(&a_surface).expect("Alice renders a capture node");

    let before = alice.multi_stage_phase();
    report_decode(&mut alice, &a_surface.surface_id, &binding, bob_qr);
    let after = alice.multi_stage_phase();

    assert!(
        matches!(before, Some(MultiStagePhase::Advertising)),
        "Alice starts out advertising her own frame, got {before:?}"
    );
    assert!(
        !matches!(after, Some(MultiStagePhase::Advertising)),
        "reporting Bob's decoded frame must move Alice's machine on; staying in \
         Advertising is the bug — the payload is parsed and then dropped, got {after:?}"
    );
}

/// A camera reads on its own clock. By the time a decode reaches Core the
/// screen has often moved on by a frame: at a 100 ms sweep the shell needs
/// most of each frame to draw the new surface, and about half of the Pixel's
/// decodes were reported against the one before and dropped as stale (rig
/// runs E7, 2026-10-02). The capture node is the same camera across those
/// revisions, so its binding does not change with them.
// @internal
#[test]
fn a_decode_reported_after_the_screen_moved_on_still_advances_hover() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", clock_a.clone());
    let mut bob = engine_on_hover("Bob", clock_b);
    alice.poll_notifications();
    bob.poll_notifications();
    let drawn = latest_surface(&alice.initial_commands().expect("alice commands"))
        .expect("Alice renders a surface");
    let binding = capture_binding(&drawn).expect("Alice renders a capture node");
    let bob_qr = displayed_qr(
        &latest_surface(&bob.initial_commands().expect("bob commands")).expect("Bob renders"),
    )
    .expect("Bob displays an opening frame");

    // Alice's own code moves on to its next sweep frames before the decode
    // made against `drawn` is reported.
    let mut current = drawn.clone();
    for _ in 0..3 {
        clock_a.advance(Duration::from_millis(150));
        let _ = alice.on_wakeup();
        if let Some(surface) = latest_surface(&alice.drain_pending_commands()) {
            current = surface;
        }
    }
    assert!(
        current.revision > drawn.revision,
        "precondition: the surface moved on ({} then {})",
        drawn.revision,
        current.revision
    );
    assert_eq!(
        capture_binding(&current),
        Some(binding.clone()),
        "the camera keeps its binding across revisions"
    );

    report_decode(&mut alice, &drawn.surface_id, &binding, bob_qr);

    assert!(
        !matches!(
            alice.multi_stage_phase(),
            Some(MultiStagePhase::Advertising)
        ),
        "a decode from the same camera, one frame late, must still count, got {:?}",
        alice.multi_stage_phase()
    );
}

/// Two Hover devices aimed at each other, each decoding whatever the
/// peer displays, reach a persisted success — driven only through
/// `ReplaceSurface` out and `ValueChanged` in.
///
/// This is the hardware scenario that failed 2026-08-18:
/// `[QrScan] decoded type=INI2 len=139` repeating for 93 s against one
/// lone `[Exchange] started: Hover`.
// @scenario: exchange :: Hover completes when the shell reports decodes
#[test]
fn hover_completes_when_scans_arrive_through_the_presentation_seam() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", clock_a.clone());
    let mut bob = engine_on_hover("Bob", clock_b.clone());

    let mut alice_celebrates = 0;
    let mut bob_celebrates = 0;

    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();

        let a_cmds = alice.initial_commands().expect("alice commands");
        let b_cmds = bob.initial_commands().expect("bob commands");
        alice_celebrates += a_cmds.iter().filter(|c| is_celebrate(c)).count();
        bob_celebrates += b_cmds.iter().filter(|c| is_celebrate(c)).count();

        if let (Some(a), Some(b)) = (latest_surface(&a_cmds), latest_surface(&b_cmds)) {
            // Each side decodes whatever the peer is currently showing.
            if let (Some(binding), Some(peer_qr)) = (capture_binding(&a), displayed_qr(&b)) {
                let out = report_decode(&mut alice, &a.surface_id, &binding, peer_qr);
                alice_celebrates += out.iter().filter(|c| is_celebrate(c)).count();
            }
            if let (Some(binding), Some(peer_qr)) = (capture_binding(&b), displayed_qr(&a)) {
                let out = report_decode(&mut bob, &b.surface_id, &binding, peer_qr);
                bob_celebrates += out.iter().filter(|c| is_celebrate(c)).count();
            }
        }

        if alice.vauchi().contact_count().unwrap_or(0) > 0
            && bob.vauchi().contact_count().unwrap_or(0) > 0
        {
            break;
        }

        clock_a.advance(Duration::from_millis(500));
        clock_b.advance(Duration::from_millis(500));
    }

    assert_eq!(
        alice.vauchi().contact_count().unwrap_or(0),
        1,
        "Alice must have stored Bob's card — she reported his frames through the only \
         route a shell has. Zero is the device symptom: decode succeeds, nothing consumes it."
    );
    assert_eq!(
        bob.vauchi().contact_count().unwrap_or(0),
        1,
        "Bob must have stored Alice's card for the same reason"
    );
    assert_eq!(
        (alice_celebrates, bob_celebrates),
        (1, 1),
        "each side celebrates exactly once on validated success"
    );
}

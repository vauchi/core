// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The audio closeness check taken in turns (vauchi/private#1300).
//!
//! Two machines in a simulated room: each plays through a speaker and
//! records its own tone, loud, and its peer's, quieter, as two phones did on
//! the rig. Time is a counter in milliseconds; nothing waits (CC-06).

use vauchi_app::orchestrator::multi_stage_machine::{MultiStageEvent, MultiStageMachine};
use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_qr};
use vauchi_core::exchange::{AudioProximityState, MultiStageSession};
use vauchi_core::{Command, Event};

use crate::signed_session::signed_session;

const SAMPLE_RATE: u32 = 44_100;
const STEP_MS: u64 = 100;
const OWN_TONE_LEVEL: f32 = 0.14;
const PEER_TONE_LEVEL: f32 = 0.01;

struct Tone {
    from_first_voice: bool,
    starts_ms: u64,
    samples: Vec<f32>,
}

struct Phone {
    machine: MultiStageMachine,
    /// The camera reads nothing before this moment.
    reads_from_ms: u64,
    listening: Option<(u64, u64)>,
    listen_commands: Vec<u64>,
    outcome: Option<AudioProximityState>,
}

struct Room {
    first: Phone,
    second: Phone,
    tones: Vec<Tone>,
    now_ms: u64,
    peer_tone_level: f32,
}

fn opening_session_id(machine: &mut MultiStageMachine) -> ([u8; 16], String) {
    let MultiStageEvent::QrFrameReady(frame) = machine.advance(0) else {
        panic!("a machine shows its opening frame first");
    };
    match parse_qr(&frame.data).expect("an opening frame parses") {
        StageQr::Init { session_id, .. } => (session_id, frame.data),
        _ => panic!("the first frame is the opening frame"),
    }
}

impl Room {
    /// Two Hover phones; `first` is the one with the lower session id.
    fn new(first_reads_from_ms: u64, second_reads_from_ms: u64, peer_tone_level: f32) -> Room {
        Room::of(
            MultiStageMachine::new_hover,
            first_reads_from_ms,
            second_reads_from_ms,
            peer_tone_level,
        )
    }

    fn of(
        machine: fn(MultiStageSession, u64) -> MultiStageMachine,
        first_reads_from_ms: u64,
        second_reads_from_ms: u64,
        peer_tone_level: f32,
    ) -> Room {
        let mut a = machine(signed_session("Alice"), 0);
        let mut b = machine(signed_session("Bob"), 0);
        let (id_a, _) = opening_session_id(&mut a);
        let (id_b, _) = opening_session_id(&mut b);
        let (first, second) = if id_a < id_b { (a, b) } else { (b, a) };
        let phone = |machine, reads_from_ms| Phone {
            machine,
            reads_from_ms,
            listening: None,
            listen_commands: Vec::new(),
            outcome: None,
        };
        Room {
            first: phone(first, first_reads_from_ms),
            second: phone(second, second_reads_from_ms),
            tones: Vec::new(),
            now_ms: 0,
            peer_tone_level,
        }
    }

    fn tones_from(&self, first_voice: bool) -> Vec<&Tone> {
        self.tones
            .iter()
            .filter(|tone| tone.from_first_voice == first_voice)
            .collect()
    }

    /// What the microphone of one phone holds for a window: every tone that
    /// overlaps it, its own loud and its peer's quiet, cut at the edges.
    fn recording(&self, for_first_voice: bool, opens_ms: u64, closes_ms: u64) -> Vec<f32> {
        let per_ms = SAMPLE_RATE as u64 / 1000;
        let mut out = vec![0.0f32; ((closes_ms - opens_ms) * per_ms) as usize];
        for tone in &self.tones {
            let level = if tone.from_first_voice == for_first_voice {
                OWN_TONE_LEVEL
            } else {
                self.peer_tone_level
            };
            for (index, sample) in tone.samples.iter().enumerate() {
                let at = tone.starts_ms * per_ms + index as u64;
                if at >= opens_ms * per_ms
                    && let Some(slot) = out.get_mut((at - opens_ms * per_ms) as usize)
                {
                    *slot += sample * level;
                }
            }
        }
        out
    }

    fn run_commands(&mut self, of_first_voice: bool) {
        let now_ms = self.now_ms;
        let phone = if of_first_voice {
            &mut self.first
        } else {
            &mut self.second
        };
        let mut commands = phone.machine.try_audio_handshake_start();
        commands.extend(phone.machine.take_audio_commands());
        for command in commands {
            match command {
                Command::AudioEmitChallenge { samples, .. } => self.tones.push(Tone {
                    from_first_voice: of_first_voice,
                    starts_ms: now_ms,
                    samples,
                }),
                Command::AudioListenForResponse { timeout_ms, .. } => {
                    phone.listening = Some((now_ms, now_ms + timeout_ms));
                    phone.listen_commands.push(now_ms);
                }
                _ => {}
            }
        }
    }

    fn note(&mut self, of_first_voice: bool, event: MultiStageEvent) {
        if let MultiStageEvent::AudioProximityChanged(state) = event {
            let phone = if of_first_voice {
                &mut self.first
            } else {
                &mut self.second
            };
            phone.outcome = Some(state);
        }
        self.run_commands(of_first_voice);
    }

    /// One tick: each phone shows its next frame to the other's camera, and
    /// a phone whose window closed gets its recording.
    fn tick(&mut self) {
        let now = self.now_ms;
        let first_frame = self.first.machine.advance(now);
        let second_frame = self.second.machine.advance(now);
        self.run_commands(true);
        self.run_commands(false);
        if let MultiStageEvent::QrFrameReady(frame) = first_frame
            && now >= self.second.reads_from_ms
        {
            let event = self
                .second
                .machine
                .handle_hardware_event(&Event::QrScanned { data: frame.data }, now);
            self.note(false, event);
        }
        if let MultiStageEvent::QrFrameReady(frame) = second_frame
            && now >= self.first.reads_from_ms
        {
            let event = self
                .first
                .machine
                .handle_hardware_event(&Event::QrScanned { data: frame.data }, now);
            self.note(true, event);
        }
        for of_first_voice in [true, false] {
            let window = if of_first_voice {
                self.first.listening
            } else {
                self.second.listening
            };
            let Some((opens_ms, closes_ms)) = window.filter(|(_, closes_ms)| now >= *closes_ms)
            else {
                continue;
            };
            let samples = self.recording(of_first_voice, opens_ms, closes_ms);
            let phone = if of_first_voice {
                &mut self.first
            } else {
                &mut self.second
            };
            phone.listening = None;
            let event = phone.machine.handle_hardware_event(
                &Event::AudioSamplesRecorded {
                    samples,
                    sample_rate: SAMPLE_RATE,
                },
                now,
            );
            self.note(of_first_voice, event);
        }
        self.now_ms += STEP_MS;
    }

    /// Run until both phones have the check's result, or a minute passed.
    fn run_to_the_end(&mut self) {
        while self.now_ms < 60_000
            && (self.first.outcome.is_none() || self.second.outcome.is_none())
        {
            self.tick();
        }
    }
}

fn tone_ms(tone: &Tone) -> u64 {
    tone.samples.len() as u64 * 1000 / SAMPLE_RATE as u64
}

// @internal
#[test]
fn two_phones_that_read_each_other_two_seconds_apart_both_hear_the_other() {
    let mut room = Room::new(2_000, 0, PEER_TONE_LEVEL);

    room.run_to_the_end();

    assert_eq!(room.first.outcome, Some(AudioProximityState::Confirmed));
    assert_eq!(room.second.outcome, Some(AudioProximityState::Confirmed));
}

// @internal
#[test]
fn the_second_voice_answers_once_and_only_after_the_first_voices_tone_is_over() {
    let mut room = Room::new(0, 1_500, PEER_TONE_LEVEL);

    room.run_to_the_end();

    let calls = room.tones_from(true);
    let answers = room.tones_from(false);
    assert_eq!(answers.len(), 1);
    assert_eq!(calls.len(), 1);
    assert!(answers[0].starts_ms >= calls[0].starts_ms + tone_ms(calls[0]));
    assert_eq!(room.second.outcome, Some(AudioProximityState::Confirmed));
}

// @internal
#[test]
fn the_first_voice_plays_nothing_before_its_peer_has_read_it() {
    let never = u64::MAX;
    let mut room = Room::new(0, never, PEER_TONE_LEVEL);

    for _ in 0..200 {
        room.tick();
    }

    assert_eq!(room.tones.len(), 0);
    assert_eq!(room.first.listen_commands, Vec::<u64>::new());
    assert_eq!(room.first.outcome, None);
}

// @internal
#[test]
fn phones_that_cannot_hear_each_other_fail_after_their_rounds_and_the_second_never_answers() {
    let inaudible = 0.0;
    let mut room = Room::new(0, 0, inaudible);

    room.run_to_the_end();

    assert_eq!(room.first.outcome, Some(AudioProximityState::Failed));
    assert_eq!(room.second.outcome, Some(AudioProximityState::Failed));
    assert_eq!(room.tones_from(true).len(), 3);
    assert_eq!(room.tones_from(false).len(), 0);
    assert_eq!(room.first.listen_commands.len(), 3);
    assert_eq!(room.second.listen_commands.len(), 6);
}

// @internal
#[test]
fn whatever_the_delay_between_the_two_first_reads_both_phones_hear_each_other() {
    let mut rounds_needed = Vec::new();

    for delay_ms in (0..=6_000).step_by(250) {
        for (first_reads_from_ms, second_reads_from_ms) in [(delay_ms, 0), (0, delay_ms)] {
            let mut room = Room::new(first_reads_from_ms, second_reads_from_ms, PEER_TONE_LEVEL);
            room.run_to_the_end();
            assert_eq!(
                (room.first.outcome, room.second.outcome),
                (
                    Some(AudioProximityState::Confirmed),
                    Some(AudioProximityState::Confirmed)
                ),
                "first reads from {first_reads_from_ms} ms, second from {second_reads_from_ms} ms"
            );
            rounds_needed.push(room.tones_from(true).len());
        }
    }

    // Some delay puts the first tone across the end of a listening window;
    // without one this test would not have tried the repeat.
    assert!(rounds_needed.iter().any(|rounds| *rounds > 1));
}

// @internal
#[test]
fn malformed_recordings_use_up_the_second_voices_six_windows_and_then_the_check_fails() {
    for machine in [
        MultiStageMachine::new_hover,
        MultiStageMachine::new_tap_hover_shake,
    ] {
        let mut room = Room::of(machine, 0, 0, PEER_TONE_LEVEL);
        while room.second.listen_commands.is_empty() {
            room.tick();
        }
        let mut results = Vec::new();

        for _ in 0..6 {
            let event = room.second.machine.handle_hardware_event(
                &Event::AudioSamplesRecorded {
                    samples: vec![f32::NAN; 32],
                    sample_rate: SAMPLE_RATE,
                },
                room.now_ms,
            );
            let asked_to_listen_again = room
                .second
                .machine
                .take_audio_commands()
                .iter()
                .any(|command| matches!(command, Command::AudioListenForResponse { .. }));
            let outcome = match event {
                MultiStageEvent::AudioProximityChanged(state) => Some(state),
                _ => None,
            };
            results.push((outcome, asked_to_listen_again));
        }

        let mut expected = vec![(None, true); 5];
        expected.push((Some(AudioProximityState::Failed), false));
        assert_eq!(results, expected);
    }
}

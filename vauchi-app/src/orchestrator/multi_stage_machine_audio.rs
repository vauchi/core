// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The audio closeness check, taken in turns (vauchi/private#1300). A child
//! module because `multi_stage_machine.rs` is at the file-size limit (VRS04);
//! loaded via `#[path]` from the parent.
//!
//! Only TapHoverShake runs it. Each phone plays its session id as a tone
//! and wants to hear its peer's.
//! Two phones that each played once, the moment they first read the other,
//! never heard each other on the rig (2026-11-19): the later reader was not
//! listening yet when the earlier one played, and each recorded its own
//! tone first. So:
//!
//! - the phone with the lower session id is the first voice. It plays only
//!   after a data chunk from its peer arrived, which proves the peer has
//!   read its opening frame and is therefore already listening;
//! - the second voice only listens until it has heard the first voice, then
//!   answers once;
//! - the first voice listens through its own tone and the answer, and both
//!   look for the peer's id among every message in a recording;
//! - a round that heard nothing is repeated a bounded number of times. The
//!   two windows differ in length so that a tone cut by the edge of a
//!   window falls whole into a later one.

use vauchi_core::Command;
use vauchi_core::exchange::{AudioConfig, AudioProximityState, ProtocolState, audio_modem};

use super::{MultiStageEvent, MultiStageMachine, MultiStageMode};

const SESSION_ID_BYTES: usize = 16;
const SECOND_VOICE_WINDOW_MS: u64 = 4_000;
/// The answer starts when the second voice's window ends, at most four
/// seconds after the first voice's tone began, and lasts 1.3 seconds.
const FIRST_VOICE_WINDOW_MS: u64 = 7_000;
const FIRST_VOICE_ROUNDS: u8 = 3;
/// Long enough to listen through all of the first voice's rounds.
const SECOND_VOICE_WINDOWS: u8 = 6;

// Bound native input before resampling; allow a buffer beyond the longest
// capture this machine asks for.
const AUDIO_CAPTURE_MAX_RATE: u32 = 192_000;
const AUDIO_CAPTURE_MAX_SECONDS: u64 = FIRST_VOICE_WINDOW_MS / 1000 + 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Voice {
    First,
    Second,
}

impl Voice {
    fn rounds(self) -> u8 {
        match self {
            Voice::First => FIRST_VOICE_ROUNDS,
            Voice::Second => SECOND_VOICE_WINDOWS,
        }
    }
}

impl MultiStageMachine {
    /// Start this phone's part of the audio check when its turn has come.
    /// Called on every machine event; returns commands once.
    ///
    /// Glance and Hover return nothing. Hover's proof of closeness is the
    /// two phones reading each other's codes at the same time; it plays no
    /// tone and does not touch the microphone (owner, 2026-11-19).
    pub fn try_audio_handshake_start(&mut self) -> Vec<Command> {
        if !self.runs_the_audio_check()
            || self.inner.audio_proximity() != AudioProximityState::Pending
        {
            return Vec::new();
        }
        let Some(voice) = self.voice() else {
            return Vec::new();
        };
        if voice == Voice::First && !self.peer_is_listening() {
            return Vec::new();
        }
        if self
            .inner
            .set_audio_proximity(AudioProximityState::Listening)
            .is_err()
        {
            return Vec::new();
        }
        self.audio_rounds = 1;
        self.audio_round(voice)
    }

    /// Commands the audio check produced while handling an event: the next
    /// round, or the second voice's answer.
    pub fn take_audio_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.audio_commands)
    }

    /// One recording from the shell. The check is confirmed when it holds
    /// the peer's session id, repeated while rounds are left, and failed
    /// after the last. A recording outside a round changes nothing.
    pub(super) fn process_audio_samples(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
    ) -> MultiStageEvent {
        if !self.runs_the_audio_check()
            || self.inner.audio_proximity() != AudioProximityState::Listening
        {
            return MultiStageEvent::None;
        }
        let Some(voice) = self.voice() else {
            return MultiStageEvent::None;
        };
        if self.recording_holds_the_peers_tone(samples, sample_rate) {
            if voice == Voice::Second {
                let answer = self.own_tone(&AudioConfig::default());
                self.audio_commands.push(answer);
            }
            return self.end_the_audio_check(AudioProximityState::Confirmed);
        }
        if self.audio_rounds < voice.rounds() {
            self.audio_rounds += 1;
            let next = self.audio_round(voice);
            self.audio_commands.extend(next);
            return MultiStageEvent::None;
        }
        self.end_the_audio_check(AudioProximityState::Failed)
    }

    fn runs_the_audio_check(&self) -> bool {
        self.mode == MultiStageMode::TapHoverShake
    }

    /// Both phones know both session ids once each has read the other's
    /// opening frame, so both arrive at the same order.
    fn voice(&self) -> Option<Voice> {
        let peer = self.inner.peer_session_id()?;
        Some(if self.inner.session_id() < peer {
            Voice::First
        } else {
            Voice::Second
        })
    }

    /// A data chunk is sealed under the key of both opening frames, so one
    /// from the peer proves it has read ours, and it listens from then on.
    fn peer_is_listening(&self) -> bool {
        match self.inner.get_state() {
            ProtocolState::Transferring {
                chunks_received, ..
            } => chunks_received > 0,
            ProtocolState::Verifying | ProtocolState::Confirming | ProtocolState::Finalized => true,
            _ => false,
        }
    }

    fn audio_round(&self, voice: Voice) -> Vec<Command> {
        let config = AudioConfig::default();
        let sample_rate = config.sample_rate;
        match voice {
            Voice::First => vec![
                self.own_tone(&config),
                Command::AudioListenForResponse {
                    timeout_ms: FIRST_VOICE_WINDOW_MS,
                    sample_rate,
                },
            ],
            Voice::Second => vec![Command::AudioListenForResponse {
                timeout_ms: SECOND_VOICE_WINDOW_MS,
                sample_rate,
            }],
        }
    }

    fn own_tone(&self, config: &AudioConfig) -> Command {
        Command::AudioEmitChallenge {
            samples: audio_modem::generate_fsk_samples(&self.inner.session_id(), config),
            sample_rate: config.sample_rate,
        }
    }

    fn recording_holds_the_peers_tone(&self, samples: &[f32], sample_rate: u32) -> bool {
        let config = AudioConfig::default();
        let maximum_samples = u64::from(sample_rate)
            .checked_mul(AUDIO_CAPTURE_MAX_SECONDS)
            .and_then(|count| usize::try_from(count).ok());
        let valid_capture = sample_rate > 2 * (config.carrier_frequency + config.frequency_shift)
            && sample_rate <= AUDIO_CAPTURE_MAX_RATE
            && !samples.is_empty()
            && maximum_samples.is_some_and(|limit| samples.len() <= limit)
            && samples.iter().all(|sample| sample.is_finite());
        valid_capture
            && audio_modem::decode_fsk_messages(samples, sample_rate, &config, SESSION_ID_BYTES)
                .iter()
                .any(|message| self.inner.verify_audio_response(message) == Some(true))
    }

    fn end_the_audio_check(&mut self, state: AudioProximityState) -> MultiStageEvent {
        if self.inner.set_audio_proximity(state).is_err() {
            return MultiStageEvent::None;
        }
        MultiStageEvent::AudioProximityChanged(state)
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Pure FSK modem for ultrasonic proximity verification.
//!
//! Encodes byte payloads to PCM samples and decodes recorded samples
//! back to bytes. No I/O, no platform dependencies — both the desktop
//! CPAL backend and any future mobile-side caller share these
//! primitives.
//!
//! ## Signal Design
//!
//! - Carrier: 18.5 kHz (above human hearing for most adults)
//! - Modulation: FSK (Frequency Shift Keying)
//!   - '0' bit: carrier frequency (18.5 kHz)
//!   - '1' bit: carrier + shift (18.7 kHz)
//! - Bit duration: 10 ms (100 bps)
//! - Preamble: 50 ms at 19 kHz for synchronization

// The default build has no caller yet (Phase 3 of the audio proximity
// rewire will introduce them); gate the lint until then.
#![allow(dead_code)]

use super::proximity::ProximityError;

/// Configuration for ultrasonic audio verification.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Base carrier frequency in Hz (default: 18500 Hz)
    pub carrier_frequency: u32,
    /// Frequency shift for FSK modulation in Hz (default: 1000 Hz)
    pub frequency_shift: u32,
    /// Sample rate in Hz (default: 44100 Hz)
    pub sample_rate: u32,
    /// Minimum signal-to-noise ratio for detection (default: 15.0 dB)
    pub min_snr_db: f32,
    /// Maximum detection distance in meters (default: 3.0)
    pub max_distance_meters: f32,
    /// Duration of each FSK symbol in milliseconds (default: 20)
    pub symbol_duration_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            carrier_frequency: 18500,
            frequency_shift: 1000,
            sample_rate: 44100,
            min_snr_db: 15.0,
            max_distance_meters: 3.0,
            symbol_duration_ms: 20,
        }
    }
}

/// Generates FSK-modulated audio samples for the given data.
pub fn generate_fsk_samples(data: &[u8], config: &AudioConfig) -> Vec<f32> {
    let sample_rate = config.sample_rate as f32;
    let carrier = config.carrier_frequency as f32;
    let shift = config.frequency_shift as f32;

    // Bit duration: 10ms = 100 bps
    let samples_per_bit = (sample_rate * 0.01) as usize;

    // Preamble: 50ms at 19kHz for sync detection
    let preamble_samples = (sample_rate * 0.05) as usize;
    let preamble_freq = 19000.0;

    let mut samples = Vec::new();

    for i in 0..preamble_samples {
        let t = i as f32 / sample_rate;
        let sample = (2.0 * std::f32::consts::PI * preamble_freq * t).sin();
        samples.push(sample * 0.8); // 80% amplitude
    }

    // Small gap after preamble
    let gap_samples = (sample_rate * 0.005) as usize;
    samples.extend(vec![0.0; gap_samples]);

    for byte in data {
        for bit_idx in 0..8 {
            let bit = (byte >> (7 - bit_idx)) & 1;
            let freq = if bit == 1 { carrier + shift } else { carrier };

            for i in 0..samples_per_bit {
                let t = i as f32 / sample_rate;
                let sample = (2.0 * std::f32::consts::PI * freq * t).sin();
                samples.push(sample * 0.8);
            }
        }
    }

    samples.extend(vec![0.0; gap_samples]);

    samples
}

/// Decodes FSK-modulated audio samples back to data.
///
/// `recorded_sample_rate` is the rate the caller recorded `samples`
/// at — usually the device's native rate (44.1 kHz or 48 kHz).
/// If it differs from `config.sample_rate` the samples are
/// resampled internally before decoding.
pub fn decode_fsk_samples(
    samples: &[f32],
    recorded_sample_rate: u32,
    config: &AudioConfig,
) -> Result<Vec<u8>, ProximityError> {
    let resampled;
    let samples = if recorded_sample_rate == config.sample_rate {
        samples
    } else {
        resampled = resample(samples, recorded_sample_rate, config.sample_rate);
        &resampled
    };

    let sample_rate = config.sample_rate as f32;
    let carrier = config.carrier_frequency as f32;
    let shift = config.frequency_shift as f32;
    let samples_per_bit = (sample_rate * 0.01) as usize;

    // Find preamble (19kHz burst)
    let preamble_start = find_preamble(samples, sample_rate)?;

    let data_start = preamble_start + (sample_rate * 0.055) as usize;

    if data_start >= samples.len() {
        return Err(ProximityError::NoResponse);
    }

    let mut data = Vec::new();
    let mut current_byte = 0u8;
    let mut bit_count = 0;
    let mut sample_idx = data_start;

    while sample_idx + samples_per_bit <= samples.len() {
        let chunk = &samples[sample_idx..sample_idx + samples_per_bit];

        // Detect frequency using Goertzel algorithm
        let power_carrier = goertzel(chunk, carrier, sample_rate);
        let power_shift = goertzel(chunk, carrier + shift, sample_rate);

        let threshold = 0.01;
        if power_carrier < threshold && power_shift < threshold {
            break; // End of signal
        }

        let bit = if power_shift > power_carrier { 1 } else { 0 };

        current_byte = (current_byte << 1) | bit;
        bit_count += 1;

        if bit_count == 8 {
            data.push(current_byte);
            current_byte = 0;
            bit_count = 0;
        }

        sample_idx += samples_per_bit;
    }

    if data.is_empty() {
        return Err(ProximityError::NoResponse);
    }

    Ok(data)
}

/// Finds the start of the preamble in recorded samples.
pub fn find_preamble(samples: &[f32], sample_rate: f32) -> Result<usize, ProximityError> {
    let preamble_freq = 19000.0;
    let window_size = (sample_rate * 0.01) as usize; // 10ms windows
    let threshold = 0.05;

    for start in (0..samples.len().saturating_sub(window_size)).step_by(window_size / 2) {
        let chunk = &samples[start..start + window_size];
        let power = goertzel(chunk, preamble_freq, sample_rate);

        // The first window carrying the tone marks the start, to within one
        // window (vauchi/private#531).
        if power > threshold {
            return Ok(start);
        }
    }

    Err(ProximityError::NoResponse)
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// How far above the recording's own floor the preamble tone must stand.
/// A phone's own tone measures about 0.1 and a quiet room about 0.000001
/// (Pixel 3a recordings, vauchi/private#1300), so a level fixed in advance
/// fits neither a peer 14 cm away nor a noisy room.
const PREAMBLE_OVER_FLOOR: f32 = 30.0;
const PREAMBLE_LEVEL_MIN: f32 = 1e-6;
const PREAMBLE_HOPS: usize = 4;
/// Mean per-bit margin `|p1 - p0| / max(p0, p1)` a reading must reach.
/// The Pixel's own tone reads 0.95 at the right timing and noise about 0.3.
const MESSAGE_MARGIN_MIN: f32 = 0.6;
/// Candidate preambles read per recording, so a long interfering tone
/// cannot make a recording expensive to decode.
const MESSAGE_CANDIDATES_MAX: usize = 24;

/// Every message of `message_len` bytes in a recording, in the order heard.
///
/// A recording holds the phone's own tone as well as the peer's, so the
/// caller picks the one it waits for. Levels are measured against the
/// recording's own floor, the length is fixed, and the bit timing is
/// searched over one bit either side of where the preamble was first seen:
/// the first window that carries the preamble is up to half a window early.
pub fn decode_fsk_messages(
    samples: &[f32],
    recorded_sample_rate: u32,
    config: &AudioConfig,
    message_len: usize,
) -> Vec<Vec<u8>> {
    let resampled;
    let samples = if recorded_sample_rate == config.sample_rate {
        samples
    } else {
        resampled = resample(samples, recorded_sample_rate, config.sample_rate);
        &resampled
    };
    let sample_rate = config.sample_rate as f32;
    let bit = (sample_rate * 0.01) as usize;
    let hop = bit / 2;
    let message_bits = message_len * 8;
    if bit == 0 || hop == 0 || message_bits == 0 || samples.len() <= bit {
        return Vec::new();
    }
    let lead_in = (sample_rate * 0.055) as usize;
    let message_samples = lead_in + message_bits * bit;

    let preamble_levels: Vec<f32> = (0..samples.len() - bit)
        .step_by(hop)
        .map(|start| goertzel(&samples[start..start + bit], 19_000.0, sample_rate))
        .collect();
    let mut sorted = preamble_levels.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 4];
    let preamble_level = (floor * PREAMBLE_OVER_FLOOR).max(PREAMBLE_LEVEL_MIN);

    let mut messages = Vec::new();
    let mut candidates = 0;
    let mut index = 0;
    while index + PREAMBLE_HOPS <= preamble_levels.len() && candidates < MESSAGE_CANDIDATES_MAX {
        let preamble_here = preamble_levels[index..index + PREAMBLE_HOPS]
            .iter()
            .all(|level| *level > preamble_level);
        if !preamble_here {
            index += 1;
            continue;
        }
        candidates += 1;
        let nominal = index * hop + lead_in;
        match best_timed_reading(samples, nominal, bit, message_bits, config) {
            Some(message) => {
                messages.push(message);
                index += message_samples / hop;
            }
            None => index += PREAMBLE_HOPS,
        }
    }
    messages
}

/// The reading with the clearest bits among starts one bit either side of
/// `nominal`, or `None` when none is clear enough to be a message.
fn best_timed_reading(
    samples: &[f32],
    nominal: usize,
    bit: usize,
    message_bits: usize,
    config: &AudioConfig,
) -> Option<Vec<u8>> {
    let step = (bit / 10).max(1);
    let earliest = nominal.saturating_sub(bit);
    (earliest..=nominal + bit)
        .step_by(step)
        .filter_map(|start| read_message(samples, start, bit, message_bits, config))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, margin)| *margin >= MESSAGE_MARGIN_MIN)
        .map(|(message, _)| message)
}

/// `message_bits` bits read from `start`, with their mean margin.
fn read_message(
    samples: &[f32],
    start: usize,
    bit: usize,
    message_bits: usize,
    config: &AudioConfig,
) -> Option<(Vec<u8>, f32)> {
    let end = start.checked_add(message_bits.checked_mul(bit)?)?;
    if end > samples.len() {
        return None;
    }
    let sample_rate = config.sample_rate as f32;
    let zero = config.carrier_frequency as f32;
    let one = (config.carrier_frequency + config.frequency_shift) as f32;
    let mut message = Vec::with_capacity(message_bits / 8);
    let mut byte = 0u8;
    let mut margin_sum = 0.0f32;
    for index in 0..message_bits {
        let chunk = &samples[start + index * bit..start + (index + 1) * bit];
        let level_zero = goertzel(chunk, zero, sample_rate);
        let level_one = goertzel(chunk, one, sample_rate);
        byte = (byte << 1) | u8::from(level_one > level_zero);
        margin_sum +=
            (level_one - level_zero).abs() / level_zero.max(level_one).max(f32::MIN_POSITIVE);
        if index % 8 == 7 {
            message.push(byte);
            byte = 0;
        }
    }
    Some((message, margin_sum / message_bits as f32))
}

/// Linearly resamples `samples` from `from_rate` to `to_rate`.
///
/// Returns a new `Vec<f32>` of length approximately
/// `samples.len() * to_rate / from_rate`. When the rates match the
/// input is cloned. Suitable for narrowband ultrasonic signals where
/// the FSK band (18-20 kHz) is well below the Nyquist frequency of
/// any reasonable recording rate (44.1 kHz / 48 kHz).
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = to_rate as f32 / from_rate as f32;
    let new_count = (samples.len() as f32 * ratio) as usize;
    let mut result = vec![0.0f32; new_count];

    for (i, slot) in result.iter_mut().enumerate() {
        let src_index = i as f32 / ratio;
        let src_index_int = src_index as usize;
        let fraction = src_index - src_index_int as f32;

        if src_index_int + 1 < samples.len() {
            *slot =
                samples[src_index_int] * (1.0 - fraction) + samples[src_index_int + 1] * fraction;
        } else if src_index_int < samples.len() {
            *slot = samples[src_index_int];
        }
    }

    result
}

/// Goertzel algorithm for efficient single-frequency detection.
pub fn goertzel(samples: &[f32], target_freq: f32, sample_rate: f32) -> f32 {
    let n = samples.len();
    let k = (target_freq * n as f32 / sample_rate).round();
    let w = 2.0 * std::f32::consts::PI * k / n as f32;
    let coeff = 2.0 * w.cos();

    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;

    for &sample in samples {
        let s0 = sample + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }

    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    (power / (n * n) as f32).sqrt()
}

// INLINE_TEST_REQUIRED: Tests private Goertzel, FSK encode/decode, and preamble detection functions
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn test_fsk_encode_decode() {
        let config = AudioConfig::default();
        let data = vec![0xAB, 0xCD, 0xEF];

        let samples = generate_fsk_samples(&data, &config);

        assert!(samples.len() > 1000);

        // Decode should recover original data — recorded at the
        // same rate the modem encodes at, no resample path.
        let decoded = decode_fsk_samples(&samples, config.sample_rate, &config).unwrap();
        assert_eq!(decoded, data);
    }

    // @internal
    #[test]
    fn decode_fsk_samples_resamples_from_48k_to_44k1() {
        let config = AudioConfig::default();
        let data = vec![0x12, 0x34, 0x56];

        // Encode at the modem's native 44.1 kHz, then upsample to
        // simulate samples captured by a 48 kHz microphone.
        let native = generate_fsk_samples(&data, &config);
        let recorded_at_48k = resample(&native, config.sample_rate, 48000);

        let decoded = decode_fsk_samples(&recorded_at_48k, 48000, &config).unwrap();
        assert_eq!(decoded, data);
    }

    // @internal
    #[test]
    fn test_goertzel_detection() {
        let sample_rate = 44100.0;
        let freq = 18500.0;
        let samples: Vec<f32> = (0..4410)
            .map(|i| {
                let t = i as f32 / sample_rate;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();

        let power_target = goertzel(&samples, freq, sample_rate);
        let power_other = goertzel(&samples, 15000.0, sample_rate);

        assert!(power_target > power_other * 5.0);
    }

    // AU-6: Goertzel boundary frequency discrimination test
    // Verifies that Goertzel can distinguish the two FSK frequencies
    // (carrier vs carrier+shift) which are only 200 Hz apart.
    // @internal
    #[test]
    fn test_goertzel_boundary_fsk_discrimination() {
        let sample_rate = 44100.0;
        let carrier = 18500.0; // FSK "0" frequency
        let shift = 200.0;
        let shifted = carrier + shift; // 18700 Hz, FSK "1" frequency

        // Generate a signal at carrier frequency (18500 Hz)
        let n_samples = 4410; // 100ms at 44100 Hz
        let carrier_signal: Vec<f32> = (0..n_samples)
            .map(|i| {
                let t = i as f32 / sample_rate;
                (2.0 * std::f32::consts::PI * carrier * t).sin()
            })
            .collect();

        // Goertzel on carrier signal: should detect carrier, not shifted
        let power_at_carrier = goertzel(&carrier_signal, carrier, sample_rate);
        let power_at_shifted = goertzel(&carrier_signal, shifted, sample_rate);

        assert!(
            power_at_carrier > power_at_shifted * 3.0,
            "Carrier signal (18500 Hz): carrier power {:.6} should be 3x+ shifted power {:.6}",
            power_at_carrier,
            power_at_shifted
        );

        // Generate a signal at shifted frequency (18700 Hz)
        let shifted_signal: Vec<f32> = (0..n_samples)
            .map(|i| {
                let t = i as f32 / sample_rate;
                (2.0 * std::f32::consts::PI * shifted * t).sin()
            })
            .collect();

        // Goertzel on shifted signal: should detect shifted, not carrier
        let power_at_carrier2 = goertzel(&shifted_signal, carrier, sample_rate);
        let power_at_shifted2 = goertzel(&shifted_signal, shifted, sample_rate);

        assert!(
            power_at_shifted2 > power_at_carrier2 * 3.0,
            "Shifted signal (18700 Hz): shifted power {:.6} should be 3x+ carrier power {:.6}",
            power_at_shifted2,
            power_at_carrier2
        );
    }

    // @internal
    #[test]
    fn test_preamble_detection() {
        let config = AudioConfig::default();
        let sample_rate = config.sample_rate as f32;

        let preamble_freq = 19000.0;
        let preamble_samples = (sample_rate * 0.05) as usize;
        let mut samples: Vec<f32> = vec![0.0; 1000]; // Leading silence

        for i in 0..preamble_samples {
            let t = i as f32 / sample_rate;
            samples.push((2.0 * std::f32::consts::PI * preamble_freq * t).sin() * 0.8);
        }

        let start = find_preamble(&samples, sample_rate).unwrap();

        let one_window = (sample_rate * 0.01) as usize;
        assert!(
            (1000 - one_window..=1000).contains(&start),
            "the preamble starts at 1000, found {start}"
        );
    }

    // @internal
    #[test]
    fn resample_identity_returns_input_unchanged() {
        let samples: Vec<f32> = (0..256).map(|i| (i as f32 * 0.13).sin()).collect();
        let out = resample(&samples, 48000, 48000);
        assert_eq!(out, samples);
    }

    // @internal
    #[test]
    fn resample_empty_input_returns_empty() {
        let out = resample(&[], 48000, 44100);
        assert!(out.is_empty());
    }

    // @internal
    #[test]
    fn resample_output_length_matches_ratio() {
        // 1000 samples at 48 kHz → ~918 samples at 44.1 kHz
        let samples = vec![0.5f32; 1000];
        let out = resample(&samples, 48000, 44100);
        let expected = (1000.0 * 44100.0 / 48000.0) as usize;
        assert_eq!(out.len(), expected);
    }

    // @internal
    #[test]
    fn resample_preserves_dc_within_tolerance() {
        // A constant signal should remain (approximately) constant after
        // linear-interp resample — the only deviation is at the trailing
        // boundary where the last sample isn't interpolated.
        let samples = vec![0.7f32; 4096];
        let out = resample(&samples, 48000, 44100);

        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!(
            (mean - 0.7).abs() < 0.01,
            "DC mean should be ~0.7, got {mean}"
        );
    }

    /// A recording of `seconds`, silent but for `tones`: each is samples,
    /// the second it starts at, and its gain.
    fn recording(seconds: f32, tones: &[(&[f32], f32, f32)]) -> Vec<f32> {
        let rate = AudioConfig::default().sample_rate as f32;
        let mut out = vec![0.0f32; (seconds * rate) as usize];
        for (tone, at, gain) in tones {
            let start = (at * rate) as usize;
            for (offset, sample) in tone.iter().enumerate() {
                if let Some(slot) = out.get_mut(start + offset) {
                    *slot += sample * gain;
                }
            }
        }
        out
    }

    const OWN_ID: [u8; 16] = [0x11; 16];
    const PEER_ID: [u8; 16] = [
        0xa5, 0xbc, 0x81, 0x92, 0x4c, 0x8f, 0x74, 0xaa, 0x0a, 0xa6, 0x58, 0xc0, 0x0e, 0xf8, 0x50,
        0xf3,
    ];

    fn messages_in(recording: &[f32]) -> Vec<Vec<u8>> {
        let config = AudioConfig::default();
        decode_fsk_messages(recording, config.sample_rate, &config, 16)
    }

    // @internal
    #[test]
    fn a_recording_of_two_tones_yields_both_messages_in_the_order_heard() {
        let config = AudioConfig::default();
        let own = generate_fsk_samples(&OWN_ID, &config);
        let peer = generate_fsk_samples(&PEER_ID, &config);

        let heard = messages_in(&recording(5.0, &[(&own, 0.13, 0.14), (&peer, 1.9, 0.14)]));

        assert_eq!(heard, vec![OWN_ID.to_vec(), PEER_ID.to_vec()]);
    }

    // @internal
    #[test]
    fn a_tone_a_thousand_times_quieter_than_the_first_is_still_read() {
        let config = AudioConfig::default();
        let own = generate_fsk_samples(&OWN_ID, &config);
        let peer = generate_fsk_samples(&PEER_ID, &config);

        let heard = messages_in(&recording(
            5.0,
            &[(&own, 0.13, 0.14), (&peer, 1.6, 0.00014)],
        ));

        assert_eq!(heard, vec![OWN_ID.to_vec(), PEER_ID.to_vec()]);
    }

    // @internal
    #[test]
    fn a_tone_that_starts_between_two_bit_boundaries_is_read() {
        let config = AudioConfig::default();
        let peer = generate_fsk_samples(&PEER_ID, &config);
        let one_ms = 0.001;

        for tenths_of_a_bit in 0..10 {
            let at = 0.5 + tenths_of_a_bit as f32 * one_ms;
            let heard = messages_in(&recording(3.0, &[(&peer, at, 0.05)]));
            assert_eq!(heard, vec![PEER_ID.to_vec()], "tone at {at} s");
        }
    }

    // @internal
    #[test]
    fn a_tone_cut_off_by_the_end_of_the_recording_is_not_a_message() {
        let config = AudioConfig::default();
        let peer = generate_fsk_samples(&PEER_ID, &config);

        let heard = messages_in(&recording(2.0, &[(&peer, 1.2, 0.1)]));

        assert_eq!(heard, Vec::<Vec<u8>>::new());
    }

    // @internal
    #[test]
    fn silence_and_a_bare_whistle_hold_no_message() {
        let config = AudioConfig::default();
        let rate = config.sample_rate as f32;
        let whistle: Vec<f32> = (0..(rate * 4.0) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 19_000.0 * i as f32 / rate).sin() * 0.2)
            .collect();

        assert_eq!(messages_in(&vec![0.0; 44_100 * 3]), Vec::<Vec<u8>>::new());
        assert_eq!(messages_in(&whistle), Vec::<Vec<u8>>::new());
        assert_eq!(messages_in(&[]), Vec::<Vec<u8>>::new());
    }

    // @internal
    #[test]
    fn a_burst_at_the_preamble_pitch_with_no_data_after_it_is_not_a_message() {
        let rate = AudioConfig::default().sample_rate as f32;
        let burst: Vec<f32> = (0..(rate * 0.06) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 19_000.0 * i as f32 / rate).sin() * 0.3)
            .collect();

        let heard = messages_in(&recording(4.0, &[(&burst, 0.5, 1.0)]));

        assert_eq!(heard, Vec::<Vec<u8>>::new());
    }

    // @internal
    #[test]
    fn two_tones_on_top_of_each_other_do_not_read_as_the_quieter_one() {
        let config = AudioConfig::default();
        let own = generate_fsk_samples(&OWN_ID, &config);
        let peer = generate_fsk_samples(&PEER_ID, &config);

        let heard = messages_in(&recording(5.0, &[(&own, 0.13, 0.14), (&peer, 0.4, 0.01)]));

        assert!(!heard.contains(&PEER_ID.to_vec()));
    }

    proptest::proptest! {
        // @internal
        #[test]
        fn resample_identity_property(samples in proptest::collection::vec(-1.0f32..1.0, 0..512)) {
            let out = resample(&samples, 44100, 44100);
            proptest::prop_assert_eq!(out, samples);
        }

        // @internal
        #[test]
        fn resample_dc_preservation_property(
            level in -0.9f32..0.9,
            len in 256usize..2048,
            from_rate in 8000u32..96000,
            to_rate in 8000u32..96000,
        ) {
            let samples = vec![level; len];
            let out = resample(&samples, from_rate, to_rate);
            if out.is_empty() {
                return Ok(());
            }
            let mean = out.iter().sum::<f32>() / out.len() as f32;
            // Linear interp of a constant signal stays at that constant
            // except for boundary effects that get vanishingly small as
            // length grows.
            proptest::prop_assert!(
                (mean - level).abs() < 0.05,
                "DC mean {mean} drifted too far from {level}"
            );
        }

        // @internal
        #[test]
        fn resample_length_scales_with_ratio(
            len in 16usize..2048,
            from_rate in 8000u32..96000,
            to_rate in 8000u32..96000,
        ) {
            let samples = vec![0.0f32; len];
            let out = resample(&samples, from_rate, to_rate);
            // Implementation uses integer arithmetic (truncating
            // division). The "expected" formula in f32 can round up
            // at ratios near a sample boundary (counterexample:
            // len=807, from=44931, to=91755 → f32 rounds up to 1648,
            // integer truncates to 1647). Allow ±1 tolerance — the
            // property under test is "length scales with ratio", not
            // exact sample-count agreement with float arithmetic.
            let expected = (len as f32 * to_rate as f32 / from_rate as f32) as usize;
            let diff = (out.len() as i64 - expected as i64).abs();
            proptest::prop_assert!(
                diff <= 1,
                "out.len()={}, expected={} (|diff|={}) — len-scaling tolerance is ±1",
                out.len(),
                expected,
                diff
            );
        }
    }
}

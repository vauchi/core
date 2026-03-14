// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ambient Audio Proximity Verification (SoundProof-inspired)
//!
//! Passively records room noise on both devices simultaneously, computes
//! a SimHash fingerprint over mel-frequency energy bands, and compares
//! fingerprints. If the Hamming distance is below a threshold, the devices
//! are proven to share the same acoustic environment (co-located).
//!
//! ## How it maps to ProximityVerifier
//!
//! - `emit_challenge`: Records ambient audio and computes local fingerprint.
//!   The challenge bytes are not emitted acoustically — they serve as a
//!   session marker shared between peers.
//! - `listen_for_response`: Returns the local fingerprint (32 bytes).
//!   The peer's fingerprint arrives via the encrypted channel, not audio.
//! - `verify_response`: Compares local fingerprint with peer's fingerprint
//!   using Hamming distance. Below threshold = co-located.

use super::{ProximityConfidence, ProximityError, ProximityVerifier};
use std::sync::Mutex;
use std::time::Duration;

/// Configuration for ambient audio fingerprinting.
#[derive(Debug, Clone)]
pub struct AmbientAudioConfig {
    /// Duration to record ambient audio (ms). Default: 2000.
    pub recording_duration_ms: u32,
    /// Sample rate for recording (Hz). Default: 16000.
    pub sample_rate: u32,
    /// Number of mel-frequency bands. Default: 64.
    pub num_mel_bands: usize,
    /// Fingerprint size in bits. Fixed at 256.
    pub fingerprint_bits: usize,
    /// Minimum similarity (0.0–1.0) to consider co-located. Default: 0.7.
    pub similarity_threshold: f32,
}

impl Default for AmbientAudioConfig {
    fn default() -> Self {
        AmbientAudioConfig {
            recording_duration_ms: 2000,
            sample_rate: 16000,
            num_mel_bands: 64,
            fingerprint_bits: 256,
            similarity_threshold: 0.7,
        }
    }
}

/// 256-bit SimHash fingerprint of ambient audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFingerprint {
    bits: [u8; 32],
}

impl AudioFingerprint {
    /// All-zero fingerprint.
    pub fn zeros() -> Self {
        AudioFingerprint { bits: [0u8; 32] }
    }

    /// Create from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        AudioFingerprint { bits: bytes }
    }

    /// Access the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bits
    }

    /// Hamming distance (number of differing bits).
    pub fn hamming_distance(&self, other: &Self) -> u32 {
        self.bits
            .iter()
            .zip(other.bits.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum()
    }

    /// Similarity as a fraction (1.0 = identical, 0.0 = opposite).
    pub fn similarity(&self, other: &Self) -> f32 {
        1.0 - (self.hamming_distance(other) as f32 / 256.0)
    }

    /// Compute a SimHash fingerprint from raw audio samples.
    ///
    /// Algorithm:
    /// 1. Split samples into overlapping frames (frame_size = sample_rate / 16)
    /// 2. For each frame, compute energy in `num_mel_bands` frequency bands
    ///    using a simplified mel-scale filterbank
    /// 3. SimHash: for each of the 256 output bits, compute a weighted sum
    ///    of band energies using a deterministic hash, set bit if sum > 0
    pub fn compute(samples: &[f32], config: &AmbientAudioConfig) -> Self {
        let frame_size = (config.sample_rate as usize) / 16; // ~62.5ms frames at 16kHz
        let hop_size = frame_size / 2;
        let num_frames = if samples.len() >= frame_size {
            (samples.len() - frame_size) / hop_size + 1
        } else {
            0
        };

        if num_frames == 0 {
            return Self::zeros();
        }

        // Compute energy per mel band per frame
        let mut band_energies = vec![vec![0.0f64; config.num_mel_bands]; num_frames];
        for (frame_idx, frame_energies) in band_energies.iter_mut().enumerate().take(num_frames) {
            let start = frame_idx * hop_size;
            let frame = &samples[start..start + frame_size];
            compute_mel_band_energies(frame, config.num_mel_bands, frame_size, frame_energies);
        }

        // SimHash: accumulate weighted votes across frames
        let mut vote_counts = [0i64; 256];
        for frame_energies in &band_energies {
            for (band_idx, &energy) in frame_energies.iter().enumerate() {
                // Each band votes on all 256 bits using a deterministic hash
                let weight = (energy * 1000.0) as i64; // Scale for integer arithmetic
                for (bit_idx, vote) in vote_counts.iter_mut().enumerate() {
                    // Deterministic hash: band_idx and bit_idx determine the sign
                    let hash = simple_hash(band_idx as u32, bit_idx as u32);
                    if hash & 1 == 1 {
                        *vote += weight;
                    } else {
                        *vote -= weight;
                    }
                }
            }
        }

        // Convert vote counts to bits
        let mut bits = [0u8; 32];
        for (bit_idx, &vote) in vote_counts.iter().enumerate() {
            if vote > 0 {
                bits[bit_idx / 8] |= 1 << (bit_idx % 8);
            }
        }

        AudioFingerprint { bits }
    }
}

/// Simple deterministic hash for SimHash weight assignment.
/// Uses a mixing function — not cryptographic, just needs uniform distribution.
fn simple_hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E3779B9).wrapping_add(b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x45D9F3B);
    h ^= h >> 16;
    h
}

/// Compute energy in each mel-frequency band for a single audio frame.
///
/// Simplified mel-scale: linearly spaced bands in mel-space, mapped back to
/// frequency bins. Energy is the sum of squared magnitudes in each band.
/// Uses the real-valued DFT approximation (sum of windowed sample products).
fn compute_mel_band_energies(
    frame: &[f32],
    num_bands: usize,
    frame_size: usize,
    energies: &mut [f64],
) {
    // Frequency range: 300 Hz to 8000 Hz (speech + room noise bands)
    let mel_low = hz_to_mel(300.0);
    let mel_high = hz_to_mel(8000.0);

    for (band_idx, energy) in energies.iter_mut().enumerate().take(num_bands) {
        // Mel-space boundaries for this band
        let mel_start = mel_low + (mel_high - mel_low) * (band_idx as f64 / num_bands as f64);
        let mel_end = mel_low + (mel_high - mel_low) * ((band_idx + 1) as f64 / num_bands as f64);

        let freq_start = mel_to_hz(mel_start);
        let freq_end = mel_to_hz(mel_end);

        // Goertzel-like energy estimation for this frequency range
        *energy = goertzel_band_energy(frame, frame_size, freq_start, freq_end, 16000.0);
    }
}

/// Estimate energy in a frequency band using averaged Goertzel magnitudes.
fn goertzel_band_energy(
    frame: &[f32],
    n: usize,
    freq_low: f64,
    freq_high: f64,
    sample_rate: f64,
) -> f64 {
    // Sample 4 frequencies across the band
    let num_probes = 4;
    let mut total_energy = 0.0;
    for i in 0..num_probes {
        let freq = freq_low + (freq_high - freq_low) * (i as f64 / num_probes as f64);
        let k = (freq * n as f64 / sample_rate).round();
        let w = 2.0 * std::f64::consts::PI * k / n as f64;
        let coeff = 2.0 * w.cos();

        let mut s0 = 0.0f64;
        let mut s1 = 0.0f64;
        let mut s2;
        for &sample in frame.iter().take(n) {
            s2 = s1;
            s1 = s0;
            s0 = sample as f64 + coeff * s1 - s2;
        }
        let power = s0 * s0 + s1 * s1 - coeff * s0 * s1;
        total_energy += power.abs();
    }
    total_energy / num_probes as f64
}

/// Convert frequency (Hz) to mel scale.
fn hz_to_mel(hz: f64) -> f64 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Convert mel scale to frequency (Hz).
fn mel_to_hz(mel: f64) -> f64 {
    700.0 * (10.0f64.powf(mel / 2595.0) - 1.0)
}

/// Platform-specific ambient audio recording backend.
pub trait AmbientAudioBackend: Send + Sync {
    /// Record ambient audio samples for the given duration.
    /// Returns mono f32 samples at the configured sample rate.
    fn record_ambient(
        &self,
        duration: Duration,
        sample_rate: u32,
    ) -> Result<Vec<f32>, ProximityError>;

    /// Whether the device has a microphone available.
    fn is_available(&self) -> bool;
}

/// Ambient audio proximity verifier.
///
/// Records room noise, computes a SimHash fingerprint, and compares with
/// the peer's fingerprint to verify co-location.
pub struct AmbientAudioVerifier {
    backend: Box<dyn AmbientAudioBackend>,
    config: AmbientAudioConfig,
    local_fingerprint: Mutex<Option<AudioFingerprint>>,
}

impl AmbientAudioVerifier {
    /// Create a new verifier with default config.
    pub fn new(backend: Box<dyn AmbientAudioBackend>) -> Self {
        AmbientAudioVerifier {
            backend,
            config: AmbientAudioConfig::default(),
            local_fingerprint: Mutex::new(None),
        }
    }

    /// Create a verifier with custom config.
    pub fn with_config(backend: Box<dyn AmbientAudioBackend>, config: AmbientAudioConfig) -> Self {
        AmbientAudioVerifier {
            backend,
            config,
            local_fingerprint: Mutex::new(None),
        }
    }
}

impl ProximityVerifier for AmbientAudioVerifier {
    fn confidence_level(&self) -> ProximityConfidence {
        // Ambient audio fingerprinting provides High confidence:
        // if the room noise matches, the devices share the same acoustic space.
        ProximityConfidence::High
    }

    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        if !self.backend.is_available() {
            return Err(ProximityError::NotSupported);
        }

        let duration = Duration::from_millis(self.config.recording_duration_ms as u64);
        let samples = self
            .backend
            .record_ambient(duration, self.config.sample_rate)?;
        let fingerprint = AudioFingerprint::compute(&samples, &self.config);

        *self.local_fingerprint.lock().expect("mutex poisoned") = Some(fingerprint);

        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        let guard = self.local_fingerprint.lock().expect("mutex poisoned");
        match &*guard {
            Some(fp) => Ok(fp.as_bytes().to_vec()),
            None => Err(ProximityError::NoResponse),
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], response: &[u8]) -> bool {
        if response.len() != 32 {
            return false;
        }

        let guard = self.local_fingerprint.lock().expect("mutex poisoned");
        let local = match &*guard {
            Some(fp) => fp,
            None => return false,
        };

        let mut peer_bytes = [0u8; 32];
        peer_bytes.copy_from_slice(response);
        let peer_fp = AudioFingerprint::from_bytes(peer_bytes);

        local.similarity(&peer_fp) >= self.config.similarity_threshold
    }
}

/// Mock ambient audio backend for testing.
pub struct MockAmbientAudioBackend {
    available: bool,
    /// Pre-computed samples to return from record_ambient
    samples: Vec<f32>,
}

impl MockAmbientAudioBackend {
    /// Creates a mock that simulates co-located devices (returns consistent samples).
    pub fn co_located() -> Self {
        // Generate a deterministic "room noise" signal
        let sample_rate = 16000;
        let duration_samples = sample_rate * 2; // 2 seconds
        let samples: Vec<f32> = (0..duration_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                // Mix of low-frequency room hum + mid-frequency chatter
                (t * 120.0 * std::f32::consts::TAU).sin() * 0.3
                    + (t * 440.0 * std::f32::consts::TAU).sin() * 0.1
                    + (t * 2000.0 * std::f32::consts::TAU).sin() * 0.05
            })
            .collect();
        MockAmbientAudioBackend {
            available: true,
            samples,
        }
    }

    /// Creates a mock that simulates remote devices (returns different samples).
    pub fn remote() -> Self {
        let sample_rate = 16000;
        let duration_samples = sample_rate * 2;
        let samples: Vec<f32> = (0..duration_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                // Completely different spectral content
                (t * 5000.0 * std::f32::consts::TAU).sin() * 0.8
                    + (t * 7500.0 * std::f32::consts::TAU).sin() * 0.5
            })
            .collect();
        MockAmbientAudioBackend {
            available: true,
            samples,
        }
    }

    /// Creates a mock with no microphone available.
    pub fn unavailable() -> Self {
        MockAmbientAudioBackend {
            available: false,
            samples: Vec::new(),
        }
    }
}

impl AmbientAudioBackend for MockAmbientAudioBackend {
    fn record_ambient(
        &self,
        _duration: Duration,
        _sample_rate: u32,
    ) -> Result<Vec<f32>, ProximityError> {
        if !self.available {
            return Err(ProximityError::NotSupported);
        }
        Ok(self.samples.clone())
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

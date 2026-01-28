// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile Audio Proximity Verification
//!
//! Provides a callback interface for platform-specific audio implementations.
//! iOS uses AVAudioEngine, Android uses AudioRecord/AudioTrack.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vauchi_core::exchange::{AudioBackend, AudioCapability, AudioConfig, ProximityError};

/// Callback interface for platform-specific audio operations.
///
/// Implement this trait in Swift (iOS) or Kotlin (Android) to provide
/// native audio functionality for ultrasonic proximity verification.
#[uniffi::export(callback_interface)]
pub trait PlatformAudioHandler: Send + Sync {
    /// Check if the device supports ultrasonic audio.
    ///
    /// Returns: "full", "emit_only", "receive_only", or "none"
    fn check_capability(&self) -> String;

    /// Emit an ultrasonic signal encoding the given data.
    ///
    /// The data is already FSK-encoded samples at the configured sample rate.
    /// Platform should play these samples through the speaker.
    ///
    /// Returns empty string on success, error message on failure.
    fn emit_signal(&self, samples: Vec<f32>, sample_rate: u32) -> String;

    /// Record audio and return samples.
    ///
    /// Record for up to `timeout_ms` milliseconds at the given sample rate.
    /// Return the recorded samples as f32 values normalized to [-1.0, 1.0].
    ///
    /// Returns recorded samples, or empty vec on timeout/error.
    fn receive_signal(&self, timeout_ms: u64, sample_rate: u32) -> Vec<f32>;

    /// Check if audio is currently active.
    fn is_active(&self) -> bool;

    /// Stop any ongoing audio operation.
    fn stop(&self);
}

/// Audio backend that delegates to platform-specific implementation.
pub struct PlatformAudioBackend {
    handler: Arc<dyn PlatformAudioHandler>,
    capability: AudioCapability,
}

impl PlatformAudioBackend {
    /// Creates a new platform audio backend with the given handler.
    pub fn new(handler: Arc<dyn PlatformAudioHandler>) -> Self {
        let cap_str = handler.check_capability();
        let capability = match cap_str.as_str() {
            "full" => AudioCapability::Full,
            "emit_only" => AudioCapability::EmitOnly,
            "receive_only" => AudioCapability::ReceiveOnly,
            _ => AudioCapability::None,
        };

        PlatformAudioBackend {
            handler,
            capability,
        }
    }

    /// Generate FSK samples for the given data (same algorithm as CPAL backend).
    fn generate_fsk_samples(data: &[u8], config: &AudioConfig) -> Vec<f32> {
        let sample_rate = config.sample_rate as f32;
        let carrier = config.carrier_frequency as f32;
        let shift = config.frequency_shift as f32;

        let samples_per_bit = (sample_rate * 0.01) as usize;
        let preamble_samples = (sample_rate * 0.05) as usize;
        let preamble_freq = 19000.0;

        let mut samples = Vec::new();

        // Preamble
        for i in 0..preamble_samples {
            let t = i as f32 / sample_rate;
            let sample = (2.0 * std::f32::consts::PI * preamble_freq * t).sin();
            samples.push(sample * 0.8);
        }

        // Gap
        let gap_samples = (sample_rate * 0.005) as usize;
        samples.extend(vec![0.0; gap_samples]);

        // FSK data
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

    /// Decode FSK samples back to data.
    fn decode_fsk_samples(
        samples: &[f32],
        config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError> {
        let sample_rate = config.sample_rate as f32;
        let carrier = config.carrier_frequency as f32;
        let shift = config.frequency_shift as f32;
        let samples_per_bit = (sample_rate * 0.01) as usize;

        let preamble_start = Self::find_preamble(samples, sample_rate)?;
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

            let power_carrier = Self::goertzel(chunk, carrier, sample_rate);
            let power_shift = Self::goertzel(chunk, carrier + shift, sample_rate);

            let threshold = 0.01;
            if power_carrier < threshold && power_shift < threshold {
                break;
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

    fn find_preamble(samples: &[f32], sample_rate: f32) -> Result<usize, ProximityError> {
        let preamble_freq = 19000.0;
        let window_size = (sample_rate * 0.01) as usize;
        let threshold = 0.05;

        for start in (0..samples.len().saturating_sub(window_size)).step_by(window_size / 2) {
            let chunk = &samples[start..start + window_size];
            let power = Self::goertzel(chunk, preamble_freq, sample_rate);

            if power > threshold {
                return Ok(start);
            }
        }

        Err(ProximityError::NoResponse)
    }

    fn goertzel(samples: &[f32], target_freq: f32, sample_rate: f32) -> f32 {
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
}

impl AudioBackend for PlatformAudioBackend {
    fn check_capability(&self) -> AudioCapability {
        self.capability.clone()
    }

    fn emit_signal(&self, data: &[u8], config: &AudioConfig) -> Result<(), ProximityError> {
        if self.capability == AudioCapability::None
            || self.capability == AudioCapability::ReceiveOnly
        {
            return Err(ProximityError::NotSupported);
        }

        let samples = Self::generate_fsk_samples(data, config);
        let result = self.handler.emit_signal(samples, config.sample_rate);

        if result.is_empty() {
            Ok(())
        } else {
            Err(ProximityError::HardwareError(result))
        }
    }

    fn receive_signal(
        &self,
        timeout: Duration,
        config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError> {
        if self.capability == AudioCapability::None || self.capability == AudioCapability::EmitOnly
        {
            return Err(ProximityError::NotSupported);
        }

        let samples = self
            .handler
            .receive_signal(timeout.as_millis() as u64, config.sample_rate);

        if samples.is_empty() {
            return Err(ProximityError::Timeout);
        }

        Self::decode_fsk_samples(&samples, config)
    }

    fn is_active(&self) -> bool {
        self.handler.is_active()
    }

    fn stop(&self) {
        self.handler.stop();
    }
}

/// Result of proximity verification.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileProximityResult {
    /// Whether verification succeeded
    pub success: bool,
    /// Error message if failed
    pub error: String,
}

/// Mobile-friendly proximity verification API.
#[derive(uniffi::Object)]
pub struct MobileProximityVerifier {
    backend: Mutex<Option<PlatformAudioBackend>>,
    config: AudioConfig,
}

#[uniffi::export]
impl MobileProximityVerifier {
    /// Create a new proximity verifier with a platform audio handler.
    #[uniffi::constructor]
    pub fn new(handler: Box<dyn PlatformAudioHandler>) -> Arc<Self> {
        let backend = PlatformAudioBackend::new(Arc::from(handler));
        Arc::new(MobileProximityVerifier {
            backend: Mutex::new(Some(backend)),
            config: AudioConfig::default(),
        })
    }

    /// Create a proximity verifier without an audio handler.
    ///
    /// Will report as unsupported until a handler is provided.
    #[uniffi::constructor]
    pub fn without_handler() -> Arc<Self> {
        Arc::new(MobileProximityVerifier {
            backend: Mutex::new(None),
            config: AudioConfig::default(),
        })
    }

    /// Check if proximity verification is supported.
    pub fn is_supported(&self) -> bool {
        self.backend
            .lock()
            .unwrap()
            .as_ref()
            .map(|b| b.capability != AudioCapability::None)
            .unwrap_or(false)
    }

    /// Get device capability.
    ///
    /// Returns: "full", "emit_only", "receive_only", or "none"
    pub fn get_capability(&self) -> String {
        self.backend
            .lock()
            .unwrap()
            .as_ref()
            .map(|b| match b.capability {
                AudioCapability::Full => "full",
                AudioCapability::EmitOnly => "emit_only",
                AudioCapability::ReceiveOnly => "receive_only",
                AudioCapability::None => "none",
            })
            .unwrap_or("none")
            .to_string()
    }

    /// Emit a proximity challenge.
    ///
    /// The challenge should be 16 bytes from the QR code.
    pub fn emit_challenge(&self, challenge: Vec<u8>) -> MobileProximityResult {
        let guard = self.backend.lock().unwrap();
        let backend = match guard.as_ref() {
            Some(b) => b,
            None => {
                return MobileProximityResult {
                    success: false,
                    error: "Audio handler not set".to_string(),
                }
            }
        };

        // Encode challenge with checksum
        let mut encoded = Vec::with_capacity(18);
        encoded.push(17u8); // Length
        encoded.extend(&challenge[..challenge.len().min(16)]);
        while encoded.len() < 17 {
            encoded.push(0);
        }
        let checksum: u8 = encoded[1..17].iter().fold(0, |acc, &b| acc ^ b);
        encoded.push(checksum);

        match backend.emit_signal(&encoded, &self.config) {
            Ok(()) => MobileProximityResult {
                success: true,
                error: String::new(),
            },
            Err(e) => MobileProximityResult {
                success: false,
                error: e.to_string(),
            },
        }
    }

    /// Listen for a proximity response.
    ///
    /// Returns the received challenge bytes, or empty on timeout.
    pub fn listen_for_response(&self, timeout_ms: u64) -> Vec<u8> {
        let guard = self.backend.lock().unwrap();
        let backend = match guard.as_ref() {
            Some(b) => b,
            None => return Vec::new(),
        };

        let timeout = Duration::from_millis(timeout_ms);
        match backend.receive_signal(timeout, &self.config) {
            Ok(data) => {
                // Decode: skip length byte, extract challenge, verify checksum
                if data.len() >= 18 && data[0] == 17 {
                    let challenge = &data[1..17];
                    let checksum = data[17];
                    let computed: u8 = challenge.iter().fold(0, |acc, &b| acc ^ b);
                    if computed == checksum {
                        return challenge.to_vec();
                    }
                }
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Stop any ongoing audio operation.
    pub fn stop(&self) {
        if let Some(backend) = self.backend.lock().unwrap().as_ref() {
            backend.stop();
        }
    }
}

impl Default for MobileProximityVerifier {
    fn default() -> Self {
        MobileProximityVerifier {
            backend: Mutex::new(None),
            config: AudioConfig::default(),
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Accelerometer Tap/Shake Proximity Verification
//!
//! Both devices record accelerometer data during a shared physical action
//! (table tap, phone bump, shake pattern). Cross-correlation of the
//! magnitude envelopes proves the devices experienced the same impulse,
//! verifying physical co-location.
//!
//! ## How it maps to ProximityVerifier
//!
//! - `emit_challenge`: Records accelerometer data, computes magnitude envelope.
//! - `listen_for_response`: Returns serialized magnitude envelope (f32 array).
//! - `verify_response`: Cross-correlates local vs peer envelope. Above
//!   threshold = co-located.

use super::{ProximityConfidence, ProximityError, ProximityVerifier};
use std::sync::Mutex;
use std::time::Duration;

/// A single 3-axis accelerometer reading.
#[derive(Debug, Clone, Copy)]
pub struct AccelerometerSample {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl AccelerometerSample {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        AccelerometerSample { x, y, z }
    }

    /// Euclidean magnitude of the acceleration vector.
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

/// Configuration for accelerometer verification.
#[derive(Debug, Clone)]
pub struct AccelerometerConfig {
    /// Duration to record accelerometer data (ms). Default: 3000.
    pub recording_duration_ms: u32,
    /// Accelerometer sample rate (Hz). Default: 100.
    pub sample_rate_hz: u32,
    /// Minimum cross-correlation coefficient to accept. Default: 0.6.
    pub correlation_threshold: f32,
}

impl Default for AccelerometerConfig {
    fn default() -> Self {
        AccelerometerConfig {
            recording_duration_ms: 3000,
            sample_rate_hz: 100,
            correlation_threshold: 0.6,
        }
    }
}

/// Platform-specific accelerometer backend.
pub trait AccelerometerBackend: Send + Sync {
    /// Record accelerometer samples for the given duration.
    fn record_motion(
        &self,
        duration: Duration,
        sample_rate_hz: u32,
    ) -> Result<Vec<AccelerometerSample>, ProximityError>;

    /// Whether the device has an accelerometer.
    fn is_available(&self) -> bool;
}

/// Accelerometer tap/shake proximity verifier.
pub struct AccelerometerVerifier {
    backend: Box<dyn AccelerometerBackend>,
    config: AccelerometerConfig,
    local_envelope: Mutex<Option<Vec<f32>>>,
}

impl AccelerometerVerifier {
    pub fn new(backend: Box<dyn AccelerometerBackend>) -> Self {
        AccelerometerVerifier {
            backend,
            config: AccelerometerConfig::default(),
            local_envelope: Mutex::new(None),
        }
    }

    pub fn with_config(
        backend: Box<dyn AccelerometerBackend>,
        config: AccelerometerConfig,
    ) -> Self {
        AccelerometerVerifier {
            backend,
            config,
            local_envelope: Mutex::new(None),
        }
    }
}

/// Serialize a magnitude envelope to bytes (little-endian f32 array).
fn envelope_to_bytes(envelope: &[f32]) -> Vec<u8> {
    envelope.iter().flat_map(|&v| v.to_le_bytes()).collect()
}

/// Deserialize bytes back to a magnitude envelope.
fn bytes_to_envelope(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect(),
    )
}

/// Normalized cross-correlation (Pearson correlation coefficient).
///
/// Returns a value in [-1.0, 1.0]. Signals are truncated to the shorter length.
/// Returns 0.0 for constant or empty signals.
pub fn cross_correlate(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let a = &a[..len];
    let b = &b[..len];

    let mean_a: f64 = a.iter().map(|&x| x as f64).sum::<f64>() / len as f64;
    let mean_b: f64 = b.iter().map(|&x| x as f64).sum::<f64>() / len as f64;

    let mut cov = 0.0f64;
    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;

    for i in 0..len {
        let da = a[i] as f64 - mean_a;
        let db = b[i] as f64 - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < 1e-12 {
        return 0.0; // Constant signal
    }

    (cov / denom) as f32
}

impl ProximityVerifier for AccelerometerVerifier {
    fn confidence_level(&self) -> ProximityConfidence {
        ProximityConfidence::High
    }

    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        if !self.backend.is_available() {
            return Err(ProximityError::NotSupported);
        }

        let duration = Duration::from_millis(self.config.recording_duration_ms as u64);
        let samples = self
            .backend
            .record_motion(duration, self.config.sample_rate_hz)?;

        // Compute magnitude envelope
        let envelope: Vec<f32> = samples.iter().map(|s| s.magnitude()).collect();

        *self.local_envelope.lock().expect("mutex poisoned") = Some(envelope);
        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        let guard = self.local_envelope.lock().expect("mutex poisoned");
        match &*guard {
            Some(envelope) => Ok(envelope_to_bytes(envelope)),
            None => Err(ProximityError::NoResponse),
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], response: &[u8]) -> bool {
        let peer_envelope = match bytes_to_envelope(response) {
            Some(e) => e,
            None => return false,
        };

        let guard = self.local_envelope.lock().expect("mutex poisoned");
        let local_envelope = match &*guard {
            Some(e) => e,
            None => return false,
        };

        let correlation = cross_correlate(local_envelope, &peer_envelope);
        correlation >= self.config.correlation_threshold
    }
}

/// Mock accelerometer backend for testing.
pub struct MockAccelerometerBackend {
    available: bool,
    samples: Vec<AccelerometerSample>,
}

impl MockAccelerometerBackend {
    /// Simulates co-located devices experiencing the same tap.
    pub fn co_located() -> Self {
        let sample_rate = 100;
        let duration_samples = sample_rate * 3; // 3 seconds
        let samples: Vec<AccelerometerSample> = (0..duration_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                // Simulate a tap at t=1.0s: brief spike in acceleration
                let tap = if (t - 1.0).abs() < 0.05 {
                    10.0 * (-(((t - 1.0) / 0.02).powi(2))).exp()
                } else {
                    0.0
                };
                // Baseline gravity + tap impulse
                AccelerometerSample::new(0.1 * t.sin(), tap + 0.05, 9.81 + 0.02 * t.cos())
            })
            .collect();
        MockAccelerometerBackend {
            available: true,
            samples,
        }
    }

    /// Simulates a remote device with different motion.
    pub fn remote() -> Self {
        let sample_rate = 100;
        let duration_samples = sample_rate * 3;
        let samples: Vec<AccelerometerSample> = (0..duration_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                // Walking motion — completely different from tap
                AccelerometerSample::new(
                    (t * 3.0).sin() * 2.0,
                    (t * 3.0).cos() * 2.0,
                    9.81 + (t * 6.0).sin() * 1.5,
                )
            })
            .collect();
        MockAccelerometerBackend {
            available: true,
            samples,
        }
    }

    /// No accelerometer available.
    pub fn unavailable() -> Self {
        MockAccelerometerBackend {
            available: false,
            samples: Vec::new(),
        }
    }
}

impl AccelerometerBackend for MockAccelerometerBackend {
    fn record_motion(
        &self,
        _duration: Duration,
        _sample_rate_hz: u32,
    ) -> Result<Vec<AccelerometerSample>, ProximityError> {
        if !self.available {
            return Err(ProximityError::NotSupported);
        }
        Ok(self.samples.clone())
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

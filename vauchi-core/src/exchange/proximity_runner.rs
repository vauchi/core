// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ProximityRunner — unified verifier wrapper for BLE exchange modes.
//!
//! Each BLE mode runs a different proximity verification method
//! concurrently with data exchange. ProximityRunner provides a
//! uniform interface: start → feed hardware events → get result.
//!
//! Handles the milli-g → g conversion for accelerometer data
//! (audit F4: hardware events use i32 milli-g, verifier uses f32 g).

use super::ExchangeHardwareEvent;
use super::command::ExchangeCommand;

/// Which proximity method to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityMethod {
    /// Ultrasonic audio challenge-response (Magic mode).
    Audio,
    /// Accelerometer cross-correlation (Shake mode).
    Accelerometer,
    /// Impact peak detection (Bump mode).
    Impact,
}

/// Result of proximity verification.
#[derive(Debug, Clone)]
pub struct ProximityRunnerResult {
    /// Method that produced this result.
    pub method: ProximityMethod,
    /// Confidence score 0.0–1.0 (capped per method).
    pub confidence: f32,
    /// Whether verification succeeded.
    pub verified: bool,
}

/// Unified proximity verification runner.
///
/// Wraps a specific verification method, emits hardware commands
/// to start verification, consumes hardware events, and produces
/// a result when verification completes or times out.
pub struct ProximityRunner {
    method: ProximityMethod,
    /// Accumulated accelerometer samples (Shake/Impact modes).
    accel_samples: Vec<f32>,
    /// Whether the runner has produced a final result.
    done: bool,
    /// Final result (if done).
    result: Option<ProximityRunnerResult>,
    /// Whether accelerometer recording is complete (Shake mode).
    recording_done: bool,
}

impl ProximityRunner {
    /// Create a new runner for the given method.
    pub fn new(method: ProximityMethod) -> Self {
        Self {
            method,
            accel_samples: Vec::new(),
            done: false,
            result: None,
            recording_done: false,
        }
    }

    /// Emit hardware commands to start this proximity method.
    pub fn start(&self) -> Vec<ExchangeCommand> {
        match self.method {
            ProximityMethod::Audio => {
                let modem_config = crate::exchange::audio_modem::AudioConfig::default();
                let challenge = vec![0u8; 16]; // Populated from session key in Phase 1
                let samples =
                    crate::exchange::audio_modem::generate_fsk_samples(&challenge, &modem_config);
                vec![
                    ExchangeCommand::AudioEmitChallenge {
                        samples,
                        sample_rate: modem_config.sample_rate,
                    },
                    ExchangeCommand::AudioListenForResponse {
                        timeout_ms: 5000,
                        sample_rate: modem_config.sample_rate,
                    },
                ]
            }
            ProximityMethod::Accelerometer | ProximityMethod::Impact => {
                vec![ExchangeCommand::AccelerometerStart]
            }
        }
    }

    /// Feed a hardware event. Returns commands to emit (if any).
    ///
    /// Handles milli-g → g conversion (audit F4).
    pub fn feed_event(&mut self, event: &ExchangeHardwareEvent) -> Vec<ExchangeCommand> {
        if self.done {
            return vec![];
        }

        match (self.method, event) {
            // Audio: samples received — decode and verify
            (
                ProximityMethod::Audio,
                ExchangeHardwareEvent::AudioSamplesRecorded {
                    samples,
                    sample_rate,
                },
            ) => {
                let modem_config = crate::exchange::audio_modem::AudioConfig::default();
                let decoded = crate::exchange::audio_modem::decode_fsk_samples(
                    samples,
                    *sample_rate,
                    &modem_config,
                );
                let verified = decoded.as_ref().is_ok_and(|d| !d.is_empty());
                self.result = Some(ProximityRunnerResult {
                    method: self.method,
                    confidence: if verified { 0.85 } else { 0.0 },
                    verified,
                });
                self.done = true;
                vec![ExchangeCommand::AudioStop]
            }

            // Accelerometer: accumulate samples for correlation
            (
                ProximityMethod::Accelerometer,
                ExchangeHardwareEvent::AccelerometerData {
                    x_milli_g,
                    y_milli_g,
                    z_milli_g,
                    ..
                },
            ) => {
                // Audit F4: convert milli-g (i32) → g (f32)
                let x = *x_milli_g as f32 / 1000.0;
                let y = *y_milli_g as f32 / 1000.0;
                let z = *z_milli_g as f32 / 1000.0;
                let magnitude = (x * x + y * y + z * z).sqrt();
                self.accel_samples.push(magnitude);
                vec![]
            }

            // Impact: detect peak from ImpactDetected event
            (
                ProximityMethod::Impact,
                ExchangeHardwareEvent::ImpactDetected {
                    magnitude_milli_g, ..
                },
            ) => {
                // Audit F4: convert milli-g → g
                let magnitude_g = *magnitude_milli_g as f32 / 1000.0;
                // Confidence capped at 0.6 per spec
                let confidence = (magnitude_g / 5.0).min(0.6);
                let verified = magnitude_g >= 2.5; // Threshold per plan
                self.result = Some(ProximityRunnerResult {
                    method: self.method,
                    confidence,
                    verified,
                });
                self.done = true;
                vec![ExchangeCommand::AccelerometerStop]
            }

            _ => vec![],
        }
    }

    /// Whether the runner has finished (result available or timed out).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Get the final result (None if not yet done).
    pub fn result(&self) -> Option<&ProximityRunnerResult> {
        self.result.as_ref()
    }

    /// The method this runner is using.
    pub fn method(&self) -> ProximityMethod {
        self.method
    }

    /// Mark as timed out — produces a failed result.
    pub fn timeout(&mut self) {
        if !self.done {
            self.result = Some(ProximityRunnerResult {
                method: self.method,
                confidence: 0.0,
                verified: false,
            });
            self.done = true;
        }
    }

    /// Whether accelerometer recording is complete (Shake mode only).
    pub fn is_recording_done(&self) -> bool {
        self.recording_done
    }

    /// Finish accelerometer recording and return the encoded envelope
    /// for BLE transmission to the peer (Shake mode only).
    ///
    /// Returns `None` if not in Accelerometer mode, already done, or
    /// no samples recorded. Emits `AccelerometerStop`.
    pub fn finish_recording(&mut self) -> Option<(Vec<u8>, Vec<ExchangeCommand>)> {
        if self.method != ProximityMethod::Accelerometer
            || self.done
            || self.accel_samples.is_empty()
        {
            return None;
        }
        self.recording_done = true;
        let envelope = super::shake_protocol::encode_envelope(&self.accel_samples);
        Some((envelope, vec![ExchangeCommand::AccelerometerStop]))
    }

    /// Receive the peer's magnitude envelope and compute cross-correlation
    /// (Shake mode only). Produces the final proximity result.
    ///
    /// Returns commands to emit (empty — correlation is computed locally).
    pub fn receive_peer_envelope(&mut self, peer_data: &[u8]) -> Vec<ExchangeCommand> {
        if self.method != ProximityMethod::Accelerometer || self.done {
            return vec![];
        }

        let peer_samples = match super::shake_protocol::decode_envelope(peer_data) {
            Some(s) => s,
            None => {
                // Invalid envelope — treat as failed verification
                self.result = Some(ProximityRunnerResult {
                    method: self.method,
                    confidence: 0.0,
                    verified: false,
                });
                self.done = true;
                return vec![];
            }
        };

        // Cross-correlate: normalized dot product of magnitude envelopes
        let confidence = cross_correlate(&self.accel_samples, &peer_samples);
        let verified = confidence >= 0.3; // Threshold for shake correlation
        self.result = Some(ProximityRunnerResult {
            method: self.method,
            confidence: confidence.min(0.5), // Shake capped at 0.5 per spec
            verified,
        });
        self.done = true;
        vec![]
    }
}

/// Normalized cross-correlation of two magnitude envelopes.
///
/// Returns 0.0–1.0 where 1.0 means identical motion patterns.
/// Uses the shorter length if arrays differ in size.
fn cross_correlate(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let (a, b) = (&a[..len], &b[..len]);

    let mean_a: f32 = a.iter().sum::<f32>() / len as f32;
    let mean_b: f32 = b.iter().sum::<f32>() / len as f32;

    let mut cross = 0.0f32;
    let mut var_a = 0.0f32;
    let mut var_b = 0.0f32;

    for i in 0..len {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cross += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < f32::EPSILON {
        return 0.0;
    }

    (cross / denom).clamp(0.0, 1.0)
}

// INLINE_TEST_REQUIRED: tests exercise private struct fields (accel_samples, done, result)
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn audio_runner_starts_with_emit_challenge_and_listen() {
        let runner = ProximityRunner::new(ProximityMethod::Audio);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 2);
        assert!(matches!(
            cmds[0],
            ExchangeCommand::AudioEmitChallenge { .. }
        ));
        assert!(matches!(
            cmds[1],
            ExchangeCommand::AudioListenForResponse { .. }
        ));
    }

    // @internal
    #[test]
    fn accelerometer_runner_starts_with_accel_start() {
        let runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], ExchangeCommand::AccelerometerStart));
    }

    // @internal
    #[test]
    fn impact_runner_starts_with_accel_start() {
        let runner = ProximityRunner::new(ProximityMethod::Impact);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], ExchangeCommand::AccelerometerStart));
    }

    // @internal
    #[test]
    fn impact_runner_detects_strong_impact() {
        let mut runner = ProximityRunner::new(ProximityMethod::Impact);
        assert!(!runner.is_done());

        // 3g impact (above 2.5g threshold)
        let cmds = runner.feed_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });

        assert!(runner.is_done());
        let result = runner.result().unwrap();
        assert!(result.verified);
        assert!(result.confidence > 0.0);
        assert!(result.confidence <= 0.6); // Capped per spec
        // Should emit AccelerometerStop
        assert!(matches!(cmds[0], ExchangeCommand::AccelerometerStop));
    }

    // @internal
    #[test]
    fn impact_runner_rejects_weak_impact() {
        let mut runner = ProximityRunner::new(ProximityMethod::Impact);
        // 1g impact (below 2.5g threshold)
        runner.feed_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 1000,
        });
        let result = runner.result().unwrap();
        assert!(!result.verified);
    }

    // @internal
    #[test]
    fn milli_g_to_g_conversion_is_correct() {
        let mut runner = ProximityRunner::new(ProximityMethod::Impact);
        // 5000 milli-g = 5.0g → confidence = min(5.0/5.0, 0.6) = 0.6
        runner.feed_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 5000,
        });
        let result = runner.result().unwrap();
        assert!((result.confidence - 0.6).abs() < f32::EPSILON);
    }

    // @internal
    #[test]
    fn accelerometer_accumulates_samples() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        runner.feed_event(&ExchangeHardwareEvent::AccelerometerData {
            x_milli_g: 1000,
            y_milli_g: 0,
            z_milli_g: 0,
            timestamp_ms: 0,
        });
        runner.feed_event(&ExchangeHardwareEvent::AccelerometerData {
            x_milli_g: 0,
            y_milli_g: 2000,
            z_milli_g: 0,
            timestamp_ms: 10,
        });
        // Not done yet — needs envelope exchange (Phase 3)
        assert!(!runner.is_done());
    }

    // @internal
    #[test]
    fn audio_response_emits_audio_stop() {
        let mut runner = ProximityRunner::new(ProximityMethod::Audio);
        let modem_config = crate::exchange::audio_modem::AudioConfig::default();
        let samples = crate::exchange::audio_modem::generate_fsk_samples(&[1, 2, 3], &modem_config);
        let cmds = runner.feed_event(&ExchangeHardwareEvent::AudioSamplesRecorded {
            samples,
            sample_rate: modem_config.sample_rate,
        });
        assert!(runner.is_done());
        assert!(runner.result().unwrap().verified);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], ExchangeCommand::AudioStop));
    }

    // @internal
    #[test]
    fn audio_empty_response_is_unverified() {
        let mut runner = ProximityRunner::new(ProximityMethod::Audio);
        runner.feed_event(&ExchangeHardwareEvent::AudioSamplesRecorded {
            samples: vec![],
            sample_rate: 44100,
        });
        assert!(runner.is_done());
        let result = runner.result().unwrap();
        assert!(!result.verified);
        assert_eq!(result.confidence, 0.0);
    }

    // @internal
    #[test]
    fn timeout_produces_failed_result() {
        let mut runner = ProximityRunner::new(ProximityMethod::Audio);
        assert!(!runner.is_done());
        runner.timeout();
        assert!(runner.is_done());
        let result = runner.result().unwrap();
        assert!(!result.verified);
        assert_eq!(result.confidence, 0.0);
    }

    // @internal
    #[test]
    fn done_runner_ignores_further_events() {
        let mut runner = ProximityRunner::new(ProximityMethod::Impact);
        runner.feed_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 3000,
        });
        assert!(runner.is_done());
        let first_confidence = runner.result().unwrap().confidence;

        // Feed another event — should be ignored
        runner.feed_event(&ExchangeHardwareEvent::ImpactDetected {
            timestamp_ms: 0,
            magnitude_milli_g: 10000,
        });
        assert_eq!(runner.result().unwrap().confidence, first_confidence);
    }

    // ── Shake envelope workflow tests ──────────────────────────────

    fn feed_shake_samples(runner: &mut ProximityRunner, count: usize) {
        for i in 0..count {
            runner.feed_event(&ExchangeHardwareEvent::AccelerometerData {
                x_milli_g: ((i as f32 * 0.1).sin() * 2000.0) as i32,
                y_milli_g: ((i as f32 * 0.1).cos() * 1500.0) as i32,
                z_milli_g: 1000,
                timestamp_ms: i as u64 * 10,
            });
        }
    }

    // @internal
    #[test]
    fn shake_finish_recording_returns_envelope() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        feed_shake_samples(&mut runner, 100);

        assert!(!runner.is_recording_done());
        let (envelope, cmds) = runner.finish_recording().unwrap();

        assert!(runner.is_recording_done());
        assert!(!runner.is_done()); // Not done until peer envelope received
        assert!(!envelope.is_empty());
        assert!(
            cmds.iter()
                .any(|c| matches!(c, ExchangeCommand::AccelerometerStop))
        );
    }

    // @internal
    #[test]
    fn shake_finish_recording_fails_without_samples() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        assert!(runner.finish_recording().is_none());
    }

    // @internal
    #[test]
    fn shake_finish_recording_fails_for_non_accel() {
        let mut runner = ProximityRunner::new(ProximityMethod::Audio);
        assert!(runner.finish_recording().is_none());
    }

    // @internal
    #[test]
    fn shake_peer_envelope_produces_result() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        feed_shake_samples(&mut runner, 100);
        let (our_envelope, _) = runner.finish_recording().unwrap();

        // Simulate peer with same data (perfect correlation)
        runner.receive_peer_envelope(&our_envelope);

        assert!(runner.is_done());
        let result = runner.result().unwrap();
        assert!(result.verified);
        assert!(result.confidence > 0.0);
        assert!(result.confidence <= 0.5); // Capped per spec
    }

    // @internal
    #[test]
    fn shake_uncorrelated_envelopes_unverified() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        feed_shake_samples(&mut runner, 100);
        runner.finish_recording().unwrap();

        // Peer envelope with constant data (no correlation with sinusoidal)
        let peer_data = crate::exchange::shake_protocol::encode_envelope(&[1.0; 100]);
        runner.receive_peer_envelope(&peer_data);

        assert!(runner.is_done());
        let result = runner.result().unwrap();
        // Constant signal has zero variance → correlation 0.0
        assert!(!result.verified);
    }

    // @internal
    #[test]
    fn shake_invalid_peer_envelope_fails() {
        let mut runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        feed_shake_samples(&mut runner, 50);
        runner.finish_recording().unwrap();

        // Invalid data (wrong version)
        runner.receive_peer_envelope(&[0xFF, 0x00]);

        assert!(runner.is_done());
        assert!(!runner.result().unwrap().verified);
    }

    // ── Cross-correlation unit tests ──────────────────────────────

    // @internal
    #[test]
    fn cross_correlate_identical_signals() {
        let a = vec![1.0, 2.0, 3.0, 2.0, 1.0];
        let r = super::cross_correlate(&a, &a);
        assert!((r - 1.0).abs() < 1e-5, "Expected ~1.0, got {r}");
    }

    // @internal
    #[test]
    fn cross_correlate_empty_signals() {
        assert_eq!(super::cross_correlate(&[], &[]), 0.0);
    }

    // @internal
    #[test]
    fn cross_correlate_constant_signals() {
        let a = vec![5.0; 10];
        let b = vec![3.0; 10];
        // Both constant → zero variance → 0.0
        assert_eq!(super::cross_correlate(&a, &b), 0.0);
    }
}

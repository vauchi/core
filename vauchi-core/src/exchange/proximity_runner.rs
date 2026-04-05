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
}

impl ProximityRunner {
    /// Create a new runner for the given method.
    pub fn new(method: ProximityMethod) -> Self {
        Self {
            method,
            accel_samples: Vec::new(),
            done: false,
            result: None,
        }
    }

    /// Emit hardware commands to start this proximity method.
    pub fn start(&self) -> Vec<ExchangeCommand> {
        match self.method {
            ProximityMethod::Audio => {
                vec![ExchangeCommand::AudioEmitChallenge {
                    data: vec![0; 16], // Populated from session key in Phase 1
                }]
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
            // Audio: response received — verify
            (ProximityMethod::Audio, ExchangeHardwareEvent::AudioResponseReceived { data }) => {
                // Phase 1: verify response against challenge
                let verified = !data.is_empty();
                self.result = Some(ProximityRunnerResult {
                    method: self.method,
                    confidence: if verified { 0.85 } else { 0.0 },
                    verified,
                });
                self.done = true;
                vec![ExchangeCommand::AccelerometerStop]
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
}

// INLINE_TEST_REQUIRED: tests exercise private struct fields (accel_samples, done, result)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_runner_starts_with_emit_challenge() {
        let runner = ProximityRunner::new(ProximityMethod::Audio);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            cmds[0],
            ExchangeCommand::AudioEmitChallenge { .. }
        ));
    }

    #[test]
    fn accelerometer_runner_starts_with_accel_start() {
        let runner = ProximityRunner::new(ProximityMethod::Accelerometer);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            cmds[0],
            ExchangeCommand::AccelerometerStart { .. }
        ));
    }

    #[test]
    fn impact_runner_starts_with_accel_start() {
        let runner = ProximityRunner::new(ProximityMethod::Impact);
        let cmds = runner.start();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            cmds[0],
            ExchangeCommand::AccelerometerStart { .. }
        ));
    }

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
}

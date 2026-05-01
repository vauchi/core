// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CPAL-based Ultrasonic Audio Backend
//!
//! Real implementation of ultrasonic proximity verification using CPAL
//! for cross-platform audio I/O.
//!
//! ## Signal Design
//!
//! - Carrier: 18.5 kHz (above human hearing for most adults)
//! - Modulation: FSK (Frequency Shift Keying)
//!   - '0' bit: carrier frequency (18.5 kHz)
//!   - '1' bit: carrier + shift (18.7 kHz)
//! - Bit duration: 10ms (100 bps)
//! - Preamble: 50ms at 19 kHz for synchronization
//!
//! ## Platform Notes
//!
//! - Requires microphone permission on all platforms
//! - Some devices may not support 18+ kHz (speaker/mic limitations)
//! - Background noise rejection via bandpass filtering

use super::audio_modem::{self, AudioConfig};
use super::proximity::ProximityError;
use crate::types::AudioCapability;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// CPAL-based audio backend for desktop platforms.
pub struct CpalAudioBackend {
    /// Cached capability check result
    capability: AudioCapability,
    /// Flag to track if currently active
    is_active: Arc<AtomicBool>,
    /// Stop signal for streams
    stop_signal: Arc<AtomicBool>,
}

impl CpalAudioBackend {
    /// Creates a new CPAL audio backend.
    ///
    /// Checks device capabilities on creation.
    pub fn new() -> Result<Self, ProximityError> {
        let capability = Self::detect_capability()?;

        Ok(CpalAudioBackend {
            capability,
            is_active: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Detects audio capability by checking available devices.
    fn detect_capability() -> Result<AudioCapability, ProximityError> {
        let host = cpal::default_host();

        let has_output = host.default_output_device().is_some();
        let has_input = host.default_input_device().is_some();

        let capability = match (has_output, has_input) {
            (true, true) => AudioCapability::Full,
            (true, false) => AudioCapability::EmitOnly,
            (false, true) => AudioCapability::ReceiveOnly,
            (false, false) => AudioCapability::None,
        };

        Ok(capability)
    }
}

impl Default for CpalAudioBackend {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| CpalAudioBackend {
            capability: AudioCapability::None,
            is_active: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl CpalAudioBackend {
    /// Returns the device's audio capability.
    pub fn check_capability(&self) -> AudioCapability {
        self.capability.clone()
    }

    /// Emits an ultrasonic signal encoding the given data.
    pub fn emit_signal(&self, data: &[u8], config: &AudioConfig) -> Result<(), ProximityError> {
        if self.capability == AudioCapability::None
            || self.capability == AudioCapability::ReceiveOnly
        {
            return Err(ProximityError::NotSupported);
        }

        self.is_active.store(true, Ordering::SeqCst);
        self.stop_signal.store(false, Ordering::SeqCst);

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| ProximityError::HardwareError("No output device".into()))?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| ProximityError::HardwareError(format!("Config error: {}", e)))?;

        // Generate samples
        let samples = audio_modem::generate_fsk_samples(data, config);
        let samples = Arc::new(Mutex::new(samples));
        let sample_idx = Arc::new(Mutex::new(0usize));
        let done = Arc::new(AtomicBool::new(false));

        let samples_clone = samples.clone();
        let sample_idx_clone = sample_idx.clone();
        let done_clone = done.clone();
        let stop_signal = self.stop_signal.clone();

        let stream = device
            .build_output_stream(
                &supported_config.into(),
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if stop_signal.load(Ordering::SeqCst) {
                        output.fill(0.0);
                        done_clone.store(true, Ordering::SeqCst);
                        return;
                    }

                    let samples_guard = samples_clone.lock().expect("mutex poisoned");
                    let mut idx = sample_idx_clone.lock().expect("mutex poisoned");

                    for sample in output.iter_mut() {
                        if *idx < samples_guard.len() {
                            *sample = samples_guard[*idx];
                            *idx += 1;
                        } else {
                            *sample = 0.0;
                            done_clone.store(true, Ordering::SeqCst);
                        }
                    }
                },
                |_err| {},
                None,
            )
            .map_err(|e| ProximityError::HardwareError(format!("Stream error: {}", e)))?;

        stream
            .play()
            .map_err(|e| ProximityError::HardwareError(format!("Play error: {}", e)))?;

        // Wait for playback to complete
        let samples_len = samples.lock().expect("mutex poisoned").len();
        let duration_ms = (samples_len as f32 / config.sample_rate as f32 * 1000.0) as u64 + 100;

        let start = std::time::Instant::now();
        while !done.load(Ordering::SeqCst) {
            if start.elapsed().as_millis() as u64 > duration_ms {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        self.is_active.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Listens for an ultrasonic signal and returns decoded data.
    pub fn receive_signal(
        &self,
        timeout: Duration,
        config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError> {
        if self.capability == AudioCapability::None || self.capability == AudioCapability::EmitOnly
        {
            return Err(ProximityError::NotSupported);
        }

        self.is_active.store(true, Ordering::SeqCst);
        self.stop_signal.store(false, Ordering::SeqCst);

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| ProximityError::HardwareError("No input device".into()))?;

        let supported_config = device
            .default_input_config()
            .map_err(|e| ProximityError::HardwareError(format!("Config error: {}", e)))?;
        let recorded_sample_rate = supported_config.sample_rate().0;

        // Buffer for recording
        let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));
        let recorded_clone = recorded.clone();
        let stop_signal = self.stop_signal.clone();

        let stream = device
            .build_input_stream(
                &supported_config.into(),
                move |input: &[f32], _: &cpal::InputCallbackInfo| {
                    if !stop_signal.load(Ordering::SeqCst) {
                        recorded_clone
                            .lock()
                            .expect("mutex poisoned")
                            .extend_from_slice(input);
                    }
                },
                |_err| {},
                None,
            )
            .map_err(|e| ProximityError::HardwareError(format!("Stream error: {}", e)))?;

        stream
            .play()
            .map_err(|e| ProximityError::HardwareError(format!("Record error: {}", e)))?;

        // Record for timeout duration, checking periodically for signal
        let start = std::time::Instant::now();
        let check_interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            std::thread::sleep(check_interval);

            // Check if we have enough data and can decode
            let samples = recorded.lock().expect("mutex poisoned");
            if samples.len() > (recorded_sample_rate as usize / 2)
                && let Ok(data) =
                    audio_modem::decode_fsk_samples(&samples, recorded_sample_rate, config)
                && !data.is_empty()
            {
                drop(samples);
                self.stop_signal.store(true, Ordering::SeqCst);
                self.is_active.store(false, Ordering::SeqCst);
                return Ok(data);
            }
        }

        self.stop_signal.store(true, Ordering::SeqCst);
        self.is_active.store(false, Ordering::SeqCst);

        // Final decode attempt
        let samples = recorded.lock().expect("mutex poisoned");
        audio_modem::decode_fsk_samples(&samples, recorded_sample_rate, config)
    }

    /// Returns true if currently emitting or receiving.
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }

    /// Stops any ongoing audio operation.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }
}

// FSK modem unit tests (encode/decode/goertzel/preamble) live alongside
// the implementation in `super::audio_modem`. This module's tests would
// require a live audio device and are deferred to integration coverage
// per ADR-031 (hardware as command/event protocol).

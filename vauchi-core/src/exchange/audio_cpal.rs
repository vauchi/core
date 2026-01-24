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

use super::{AudioBackend, AudioCapability, AudioConfig, ProximityError};
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

    /// Generates FSK-modulated audio samples for the given data.
    fn generate_fsk_samples(data: &[u8], config: &AudioConfig) -> Vec<f32> {
        let sample_rate = config.sample_rate as f32;
        let carrier = config.carrier_frequency as f32;
        let shift = config.frequency_shift as f32;

        // Bit duration: 10ms = 100 bps
        let samples_per_bit = (sample_rate * 0.01) as usize;

        // Preamble: 50ms at 19kHz for sync detection
        let preamble_samples = (sample_rate * 0.05) as usize;
        let preamble_freq = 19000.0;

        let mut samples = Vec::new();

        // Generate preamble
        for i in 0..preamble_samples {
            let t = i as f32 / sample_rate;
            let sample = (2.0 * std::f32::consts::PI * preamble_freq * t).sin();
            samples.push(sample * 0.8); // 80% amplitude
        }

        // Small gap after preamble
        let gap_samples = (sample_rate * 0.005) as usize;
        samples.extend(vec![0.0; gap_samples]);

        // Generate FSK data
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

        // Trailing silence
        samples.extend(vec![0.0; gap_samples]);

        samples
    }

    /// Decodes FSK-modulated audio samples back to data.
    fn decode_fsk_samples(
        samples: &[f32],
        config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError> {
        let sample_rate = config.sample_rate as f32;
        let carrier = config.carrier_frequency as f32;
        let shift = config.frequency_shift as f32;
        let samples_per_bit = (sample_rate * 0.01) as usize;

        // Find preamble (19kHz burst)
        let preamble_start = Self::find_preamble(samples, sample_rate)?;

        // Skip preamble + gap
        let data_start = preamble_start + (sample_rate * 0.055) as usize;

        if data_start >= samples.len() {
            return Err(ProximityError::NoResponse);
        }

        let mut data = Vec::new();
        let mut current_byte = 0u8;
        let mut bit_count = 0;
        let mut sample_idx = data_start;

        // Decode until we run out of samples or detect silence
        while sample_idx + samples_per_bit <= samples.len() {
            let chunk = &samples[sample_idx..sample_idx + samples_per_bit];

            // Detect frequency using Goertzel algorithm
            let power_carrier = Self::goertzel(chunk, carrier, sample_rate);
            let power_shift = Self::goertzel(chunk, carrier + shift, sample_rate);

            // Check if signal is present (above noise floor)
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
    fn find_preamble(samples: &[f32], sample_rate: f32) -> Result<usize, ProximityError> {
        let preamble_freq = 19000.0;
        let window_size = (sample_rate * 0.01) as usize; // 10ms windows
        let threshold = 0.05;

        for start in (0..samples.len().saturating_sub(window_size)).step_by(window_size / 2) {
            let chunk = &samples[start..start + window_size];
            let power = Self::goertzel(chunk, preamble_freq, sample_rate);

            if power > threshold {
                // Found preamble, scan back to find exact start
                let scan_start = start.saturating_sub(window_size);
                for i in scan_start..start {
                    let mini_chunk = &samples[i..i.min(i + window_size / 4).min(samples.len())];
                    if Self::goertzel(mini_chunk, preamble_freq, sample_rate) > threshold / 2.0 {
                        return Ok(i);
                    }
                }
                return Ok(start);
            }
        }

        Err(ProximityError::NoResponse)
    }

    /// Goertzel algorithm for efficient single-frequency detection.
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

impl Default for CpalAudioBackend {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| CpalAudioBackend {
            capability: AudioCapability::None,
            is_active: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl AudioBackend for CpalAudioBackend {
    fn check_capability(&self) -> AudioCapability {
        self.capability.clone()
    }

    fn emit_signal(&self, data: &[u8], config: &AudioConfig) -> Result<(), ProximityError> {
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
        let samples = Self::generate_fsk_samples(data, config);
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
                |err| {
                    eprintln!("Audio output error: {}", err);
                },
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

    fn receive_signal(
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
                |err| {
                    eprintln!("Audio input error: {}", err);
                },
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
            if samples.len() > (config.sample_rate as usize / 2) {
                // Try to decode - if successful, we're done
                if let Ok(data) = Self::decode_fsk_samples(&samples, config) {
                    if !data.is_empty() {
                        drop(samples);
                        self.stop_signal.store(true, Ordering::SeqCst);
                        self.is_active.store(false, Ordering::SeqCst);
                        return Ok(data);
                    }
                }
            }
        }

        self.stop_signal.store(true, Ordering::SeqCst);
        self.is_active.store(false, Ordering::SeqCst);

        // Final decode attempt
        let samples = recorded.lock().expect("mutex poisoned");
        Self::decode_fsk_samples(&samples, config)
    }

    fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }

    fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsk_encode_decode() {
        let config = AudioConfig::default();
        let data = vec![0xAB, 0xCD, 0xEF];

        let samples = CpalAudioBackend::generate_fsk_samples(&data, &config);

        // Should have preamble + gap + data + trailing
        assert!(samples.len() > 1000);

        // Decode should recover original data
        let decoded = CpalAudioBackend::decode_fsk_samples(&samples, &config).unwrap();
        assert_eq!(decoded, data);
    }

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

        let power_target = CpalAudioBackend::goertzel(&samples, freq, sample_rate);
        let power_other = CpalAudioBackend::goertzel(&samples, 15000.0, sample_rate);

        // Target frequency should have much higher power
        assert!(power_target > power_other * 5.0);
    }

    #[test]
    fn test_preamble_detection() {
        let config = AudioConfig::default();
        let sample_rate = config.sample_rate as f32;

        // Generate just a preamble
        let preamble_freq = 19000.0;
        let preamble_samples = (sample_rate * 0.05) as usize;
        let mut samples: Vec<f32> = vec![0.0; 1000]; // Leading silence

        for i in 0..preamble_samples {
            let t = i as f32 / sample_rate;
            samples.push((2.0 * std::f32::consts::PI * preamble_freq * t).sin() * 0.8);
        }

        let start = CpalAudioBackend::find_preamble(&samples, sample_rate).unwrap();
        // Should find preamble somewhere around the 1000-sample mark (after the silence)
        // Allow some tolerance due to windowing
        assert!(
            start < 1500,
            "Preamble should be found near start of signal, got {}",
            start
        );
    }
}

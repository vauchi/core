// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shake envelope exchange protocol.
//!
//! Serializes f32 magnitude envelopes (accelerometer data) into a
//! compact u8 format for BLE transmission. Each sample is quantized
//! to 1 byte (0–255 mapping to 0.0–8.0g range), keeping the payload
//! under BLE MTU limits with existing chunking infrastructure.
//!
//! Typical shake recording: ~300 samples at 100Hz for 3 seconds
//! = 300 bytes + 1 byte header = 301 bytes total.

/// Maximum acceleration value in g for quantization range.
/// Values above this are clamped.
const MAX_G: f32 = 8.0;

/// Protocol version byte prepended to envelope data.
const ENVELOPE_VERSION: u8 = 0x01;

/// Encode f32 magnitude samples into a compact byte envelope.
///
/// Each f32 sample (in g) is quantized to a single u8:
/// `byte = clamp(sample / MAX_G * 255.0, 0, 255)`
///
/// Format: `[version(1)][samples(N)]` = N+1 bytes.
pub fn encode_envelope(samples: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + samples.len());
    buf.push(ENVELOPE_VERSION);
    for &sample in samples {
        let clamped = sample.clamp(0.0, MAX_G);
        let byte = (clamped / MAX_G * 255.0) as u8;
        buf.push(byte);
    }
    buf
}

/// Decode a byte envelope back into f32 magnitude samples.
///
/// Returns `None` if the data is empty or has an unsupported version.
pub fn decode_envelope(data: &[u8]) -> Option<Vec<f32>> {
    if data.is_empty() {
        return None;
    }
    if data[0] != ENVELOPE_VERSION {
        return None;
    }
    let samples = data[1..]
        .iter()
        .map(|&byte| byte as f32 / 255.0 * MAX_G)
        .collect();
    Some(samples)
}

/// Maximum quantization error for a single sample (half-step).
pub const MAX_QUANTIZATION_ERROR: f32 = MAX_G / 255.0 / 2.0;

// INLINE_TEST_REQUIRED: tests verify private constants (MAX_G, ENVELOPE_VERSION) and quantization math
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn encode_decode_roundtrip() {
        let samples = vec![0.0, 1.0, 2.5, 5.0, 8.0];
        let encoded = encode_envelope(&samples);
        let decoded = decode_envelope(&encoded).unwrap();

        assert_eq!(decoded.len(), samples.len());
        for (original, recovered) in samples.iter().zip(decoded.iter()) {
            assert!(
                (original - recovered).abs() < MAX_G / 255.0,
                "Roundtrip error too large: {original} vs {recovered}"
            );
        }
    }

    // @internal
    #[test]
    fn encode_empty_samples() {
        let encoded = encode_envelope(&[]);
        assert_eq!(encoded.len(), 1); // Just version byte
        assert_eq!(encoded[0], ENVELOPE_VERSION);

        let decoded = decode_envelope(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    // @internal
    #[test]
    fn decode_empty_data_returns_none() {
        assert!(decode_envelope(&[]).is_none());
    }

    // @internal
    #[test]
    fn decode_wrong_version_returns_none() {
        assert!(decode_envelope(&[0xFF, 0x00]).is_none());
    }

    // @internal
    #[test]
    fn clamps_negative_values_to_zero() {
        let samples = vec![-1.0, -100.0];
        let encoded = encode_envelope(&samples);
        let decoded = decode_envelope(&encoded).unwrap();
        for val in &decoded {
            assert!((*val - 0.0).abs() < f32::EPSILON, "Expected 0.0, got {val}");
        }
    }

    // @internal
    #[test]
    fn clamps_values_above_max_g() {
        let samples = vec![10.0, 100.0];
        let encoded = encode_envelope(&samples);
        let decoded = decode_envelope(&encoded).unwrap();
        for val in &decoded {
            assert!(
                (val - MAX_G).abs() < MAX_G / 255.0,
                "Expected ~{MAX_G}, got {val}"
            );
        }
    }

    // @internal
    #[test]
    fn typical_shake_fits_ble_mtu() {
        // 300 samples at 100Hz × 3 seconds
        let samples: Vec<f32> = (0..300)
            .map(|i| (i as f32 / 100.0).sin().abs() * 3.0)
            .collect();
        let encoded = encode_envelope(&samples);
        // 301 bytes — fits BLE MTU with chunking (typical MTU 512+ bytes)
        assert_eq!(encoded.len(), 301);
    }

    // @internal
    #[test]
    fn quantization_error_within_bounds() {
        // Test across the full range
        for i in 0..=100 {
            let original = i as f32 / 100.0 * MAX_G;
            let encoded = encode_envelope(&[original]);
            let decoded = decode_envelope(&encoded).unwrap();
            let error = (original - decoded[0]).abs();
            assert!(
                error <= MAX_G / 255.0,
                "Error {error} exceeds bound for input {original}"
            );
        }
    }

    // @internal
    #[test]
    fn format_is_version_plus_raw_bytes() {
        let samples = vec![0.0, MAX_G];
        let encoded = encode_envelope(&samples);
        assert_eq!(encoded[0], ENVELOPE_VERSION);
        assert_eq!(encoded[1], 0); // 0.0g → 0
        assert_eq!(encoded[2], 255); // MAX_G → 255
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device capability types.
//!
//! Static description of what hardware a device supports.

use crate::exchange::AudioCapability;
use serde::{Deserialize, Serialize};

/// Static description of device hardware capabilities.
///
/// Populated once at app startup by the platform layer (iOS/Android/Desktop).
/// All fields use `#[serde(default)]` for backward compatibility when
/// deserializing from older versions that may not include newer fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DeviceCapabilities {
    /// Device has NFC hardware (for NFC Active exchange).
    #[serde(default)]
    pub has_nfc: bool,

    /// Device has Bluetooth Low Energy hardware.
    #[serde(default)]
    pub has_ble: bool,

    /// Device has a camera (for QR code scanning).
    #[serde(default)]
    pub has_camera: bool,

    /// Device audio capabilities (for ultrasonic proximity verification).
    #[serde(default)]
    pub audio: AudioCapability,

    /// Device supports biometric authentication.
    #[serde(default)]
    pub has_biometrics: bool,

    /// Type of biometric hardware, if available.
    #[serde(default)]
    pub biometric_type: Option<BiometricType>,

    /// Device has a secure enclave / hardware security module.
    #[serde(default)]
    pub has_secure_enclave: bool,

    /// Platform identifier.
    #[serde(default)]
    pub platform: Platform,

    /// Device has an accelerometer (for Bump/Shake proximity).
    #[serde(default)]
    pub has_accelerometer: bool,

    /// Device has internet connectivity (for relay/Link/Web modes).
    #[serde(default)]
    pub has_internet: bool,
}

/// Type of biometric hardware available on the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BiometricType {
    /// Fingerprint sensor (Touch ID, Android fingerprint).
    Fingerprint,
    /// Face recognition (Face ID, Android face unlock).
    FaceId,
    /// Iris scanner.
    Iris,
}

/// Platform the app is running on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Platform {
    Android,
    Ios,
    Desktop,
    Web,
    #[default]
    Unknown,
}

// INLINE_TEST_REQUIRED: tests verify backward-compat deserialization against DeviceCapabilities
// struct internals and serde(default) behaviour — must live alongside the type definition
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fields_default_to_false() {
        let caps = DeviceCapabilities::default();
        assert!(
            !caps.has_accelerometer,
            "has_accelerometer should default to false"
        );
        assert!(!caps.has_internet, "has_internet should default to false");
    }

    #[test]
    fn backward_compat_deserialize_without_new_fields() {
        // Old JSON that was serialized before these fields were added.
        let old_json = r#"{"has_nfc":true,"has_ble":false,"has_camera":true}"#;
        let caps: DeviceCapabilities =
            serde_json::from_str(old_json).expect("deserialize old JSON");
        assert!(
            !caps.has_accelerometer,
            "missing field should default to false"
        );
        assert!(!caps.has_internet, "missing field should default to false");
        assert!(caps.has_nfc, "existing field must still be true");
    }

    #[test]
    fn roundtrip_with_new_fields() {
        let caps = DeviceCapabilities {
            has_accelerometer: true,
            has_internet: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        let decoded: DeviceCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.has_accelerometer);
        assert!(decoded.has_internet);
    }
}

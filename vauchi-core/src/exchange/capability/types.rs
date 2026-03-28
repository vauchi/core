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

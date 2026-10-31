// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exchange session metadata enums (transport, proximity, audio, origin).
//!
//! A neutral leaf module: shared by `exchange`, `contact`, `storage`, and
//! `api` without depending on any of them, so these enums never pull the
//! heavy `exchange` module into a lightweight consumer.

/// Where an event originated — local device or synced from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum EventOrigin {
    /// Event happened on this device.
    Local,
    /// Event arrived via sync from another device.
    Synced,
}

/// Transport method used for contact exchange.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    // WHY: `contacts.exchange_transport` is `NOT NULL DEFAULT 'Qr'`, so a row
    // written without the column parses only through these aliases.
    #[serde(alias = "Qr")]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Nfc")]
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Ble")]
    Ble,
    /// USB cable exchange: TCP over physical cable connection.
    Usb,
    /// Audio data channel exchange: ultrasonic or audible payload transfer.
    Audio,
    /// Multi-stage QR family (Hover / Glance screen ritual): animated
    /// mutual-QR bootstrap. Reader support ships ahead of any writer
    /// (consolidation Step 2e) — decoding is fail-closed on unknown
    /// variants, so every released reader must know this name before
    /// the first persist path stamps it. No writer exists yet.
    MultiStage,
    /// Asynchronous link-mode exchange: relay-mediated, initiated by sharing
    /// a `vauchi://exchange?pk=…&n=…` URL. Both sides write `Link` at finalize
    /// time — labels the exchange semantics (asynchronous, relay-mediated),
    /// not the URL's delivery channel (SMS / email / messenger — unobservable).
    /// Problem record: `2026-04-27-deep-link-responder-flow`.
    Link,
}

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ProximityConfidence {
    /// High confidence: verified by ultrasonic audio or NFC tap.
    High,
    /// Medium confidence: manual user confirmation.
    Medium,
    /// Low confidence: proximity check failed or timed out.
    Low,
    /// Unknown: no proximity check was performed (legacy contacts).
    #[default]
    Unknown,
}

/// Represents device audio capabilities.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum AudioCapability {
    /// Device supports full ultrasonic audio (speaker + microphone)
    Full,
    /// Device can only emit ultrasonic audio (no microphone)
    EmitOnly,
    /// Device can only receive ultrasonic audio (no speaker)
    ReceiveOnly,
    /// Device does not support ultrasonic audio
    #[default]
    None,
}

/// Static description of device hardware capabilities.
///
/// Populated once at app startup by the platform layer (iOS/Android/Desktop).
/// All fields use `#[serde(default)]` for backward compatibility when
/// deserializing from older versions that may not include newer fields.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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

    /// The shell can seal a secret behind the operating system's own
    /// user-presence check (biometric or device credential) and answers
    /// `Command::CreateUserPresenceSecret` / `RevealUserPresenceSecret`
    /// (#712). Off until a shell implements them.
    #[serde(default)]
    pub can_seal_user_presence: bool,

    /// Device has an accelerometer (for Bump/Shake proximity).
    #[serde(default)]
    pub has_accelerometer: bool,

    /// Device has internet connectivity (for relay/Link/Web modes).
    #[serde(default)]
    pub has_internet: bool,

    /// Device has a USB port capable of data transfer (for Cable exchange).
    #[serde(default)]
    pub has_usb_port: bool,

    /// Human-readable device model name reported by the platform, used as the
    /// default name when joining an existing identity from this device.
    #[serde(default)]
    pub device_name: Option<String>,
}

/// Type of biometric hardware available on the device.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
mod capability_tests {
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

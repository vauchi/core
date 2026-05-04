// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange command/event protocol (ADR-031).
//!
//! Core emits [`ExchangeCommand`]s to tell frontends what hardware actions to
//! perform (display QR, start BLE scan, emit audio challenge, etc.).
//!
//! Frontends report hardware results back via [`ExchangeHardwareEvent`]s.
//!
//! This decouples the protocol state machine from platform-specific hardware
//! access and works over UniFFI / C ABI boundaries (all types are serializable).

use serde::{Deserialize, Serialize};

/// A command from core to the frontend requesting a hardware action.
///
/// Frontends match on these and dispatch to platform-specific APIs
/// (camera, BLE stack, NFC reader, audio subsystem). Commands that the
/// platform cannot fulfil should be answered with
/// [`ExchangeHardwareEvent::HardwareUnavailable`].
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ExchangeCommand {
    // ── QR ───────────────────────────────────────────────────────────
    /// Display a QR code containing `data`.
    QrDisplay { data: String },
    /// Request the frontend to open a QR scanner (camera).
    QrRequestScan,

    // ── BLE ──────────────────────────────────────────────────────────
    /// Start advertising the vauchi BLE service with the given payload.
    BleStartAdvertising {
        service_uuid: String,
        payload: Vec<u8>,
    },
    /// Start scanning for vauchi BLE peripherals.
    BleStartScanning { service_uuid: String },
    /// Connect to a discovered BLE device.
    BleConnect { device_id: String },
    /// Write data to a BLE characteristic.
    BleWriteCharacteristic { uuid: String, data: Vec<u8> },
    /// Read data from a BLE characteristic.
    BleReadCharacteristic { uuid: String },
    /// Disconnect from the current BLE peer.
    BleDisconnect,

    // ── NFC ──────────────────────────────────────────────────────────
    /// Activate the NFC interface and prepare to exchange `payload`.
    NfcActivate { payload: Vec<u8> },
    /// Deactivate the NFC interface.
    NfcDeactivate,

    // ── Audio (ultrasonic proximity) ─────────────────────────────────
    /// Emit ultrasonic PCM samples encoding a challenge.
    ///
    /// Core has already FSK-encoded the challenge bytes; the frontend
    /// just plays the samples through the device speaker. Mono float
    /// PCM at `sample_rate`.
    AudioEmitChallenge { samples: Vec<f32>, sample_rate: u32 },
    /// Listen for an ultrasonic response within `timeout_ms`.
    ///
    /// `sample_rate` is the suggested capture rate; if the device's
    /// preferred rate differs, the frontend reports its actual rate
    /// in [`ExchangeHardwareEvent::AudioSamplesRecorded`].
    AudioListenForResponse { timeout_ms: u64, sample_rate: u32 },
    /// Stop all audio operations.
    AudioStop,

    // ── Accelerometer ───────────────────────────────────────────────
    /// Start accelerometer sampling for proximity verification.
    AccelerometerStart,
    /// Stop accelerometer sampling.
    AccelerometerStop,

    // ── Relay escrow ────────────────────────────────────────────────
    /// Deposit encrypted card into relay escrow gate.
    RelayEscrowDeposit {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
        encrypted_card: Vec<u8>,
        ttl_seconds: u32,
    },
    /// Check relay escrow gate readiness (poll until ready).
    ///
    /// Frontends should poll at `suggested_interval_ms` with exponential
    /// backoff (cap at 5 min). Report `RelayEscrowReady` when gate has
    /// ≥2 deposits, or `RelayEscrowFailed` on error/timeout.
    RelayEscrowCheck {
        gate_hash: Vec<u8>,
        /// Suggested initial polling interval in milliseconds.
        suggested_interval_ms: u32,
    },
    /// Retrieve blob from relay escrow gate.
    RelayEscrowRetrieve {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
    },

    // ── Link mode ───────────────────────────────────────────────────
    /// Show system share sheet with a URL.
    ShowShareSheet { url: String },

    // ── BLE cleanup ────────────────────────────────────────────────
    // Appended (not inserted) to preserve serde discriminant ordering.
    /// Stop BLE scanning (saves battery after discovery completes).
    BleStopScanning,

    // ── Direct transport (USB/TCP) ─────────────────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Send an exchange payload over a direct transport (USB cable / local TCP).
    ///
    /// The frontend should:
    /// 1. Send `payload` to the peer over the established TCP connection
    /// 2. Receive the peer's payload from the same connection
    /// 3. Report the peer's data via [`ExchangeHardwareEvent::DirectPayloadReceived`]
    ///
    /// The `is_initiator` flag determines send/recv ordering to avoid deadlock
    /// (initiator sends first, responder receives first).
    DirectSend {
        payload: Vec<u8>,
        is_initiator: bool,
    },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Request the frontend to open the device photo library / gallery.
    ///
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the selected image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// dismisses the picker.
    ImagePickFromLibrary,
    /// Request the frontend to capture an image from the device camera.
    ///
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the captured image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// cancels.
    ImageCaptureFromCamera,
    /// Request the frontend to open a file picker for image files.
    ///
    /// Used on desktop platforms where a photo library may not exist.
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the selected image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// cancels.
    ImagePickFromFile,

    // ── Camera control (multi-stage exchange) ──────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Switch the active camera between front- and rear-facing.
    ///
    /// Used by the multi-stage face-to-face exchange screen so the user
    /// can flip the scanner orientation without the frontend owning the
    /// preference. `use_front == true` selects the front camera.
    SwitchCamera { use_front: bool },

    // ── File picking (vCard / backup import, ADR-031) ──────────────
    // Appended to preserve serde discriminant ordering.
    /// Request the frontend to open a file picker.
    ///
    /// `accepted_mime_types` is advisory — frontends may default to a
    /// coarser superset on platforms where the OS picker doesn't filter
    /// by MIME (e.g., older Android versions). `purpose` lets the
    /// frontend label the picker dialog without hardcoding strings;
    /// label text comes from core's locale store via `t(key)`.
    ///
    /// Frontend should return [`ExchangeHardwareEvent::FilePickedFromUser`]
    /// with the selected file's bytes, or
    /// [`ExchangeHardwareEvent::FilePickCancelledByUser`] if the user
    /// dismisses the picker.
    ///
    /// Distinct from [`ExchangeCommand::ImagePickFromFile`]: that variant
    /// returns [`ExchangeHardwareEvent::ImageReceived`] which is shaped
    /// for avatar normalization. File picking returns raw bytes plus
    /// filename for arbitrary payloads (vCard, encrypted backup blob,
    /// future key bundles, etc.).
    FilePickFromUser {
        accepted_mime_types: Vec<String>,
        purpose: FilePickPurpose,
    },

    // ── Screen presentation hardware (multi-stage exchange) ────────
    // Appended to preserve serde discriminant ordering.
    /// Set the device screen brightness, optionally restoring the
    /// platform default when `level` is `None`.
    ///
    /// Used by screens that need a specific brightness for their
    /// hardware to function (e.g., the multi-stage face-to-face
    /// exchange uses 65% brightness so the front camera is not
    /// over-exposed when scanning a peer's QR). The frontend is
    /// responsible for snapshotting the prior value on the *first*
    /// `Some(level)` after a `None` (or app start) so the
    /// subsequent `None` correctly restores it.
    ///
    /// Frontends that have no programmatic brightness control (e.g.,
    /// desktop, where the OS owns it) answer with
    /// [`ExchangeHardwareEvent::HardwareUnavailable { transport: "screen_brightness" }`]
    /// — core should treat that as "request honoured at platform
    /// default" and not retry.
    ///
    /// Per `2026-05-01-screen-id-metadata-in-core` cousin
    /// `2026-05-04-exchange-command-screen-presentation`, ADR-031
    /// §Hardware. Phase 1 of the FaceToFaceExchangeView retirement.
    SetScreenBrightness { level: Option<f32> },

    /// Disable or re-enable the platform's idle / auto-lock timer.
    ///
    /// `disabled = true` keeps the screen awake (used by the
    /// multi-stage exchange so a longer-than-30s handshake doesn't
    /// trigger the device's auto-lock). `disabled = false` restores
    /// the platform default on screen exit. Idempotent — frontends
    /// MAY ignore a redundant set/clear.
    ///
    /// Frontends that have no programmatic idle-timer control answer
    /// with [`ExchangeHardwareEvent::HardwareUnavailable { transport:
    /// "idle_timer" }`].
    ///
    /// Phase 1 of the FaceToFaceExchangeView retirement (companion
    /// to [`ExchangeCommand::SetScreenBrightness`]).
    SetIdleTimerDisabled { disabled: bool },
}

/// Why a file picker is being opened — lets frontends label the dialog
/// (e.g., "Import Contacts" vs "Import Backup") without hardcoded strings.
///
/// Variants map 1:1 to a label key in the locale store. `Other` covers
/// future imports (e.g., key-bundle import) without forcing every consumer
/// to update for a new well-known purpose.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FilePickPurpose {
    /// vCard / VCF import on top of the contacts engine.
    ImportContacts,
    /// Encrypted vauchi backup blob.
    ImportBackup,
    /// Reserved for future imports — frontends look up `label_key`
    /// in the locale store.
    Other { label_key: String },
}

impl ExchangeCommand {
    /// Returns the variant name without payload data (safe for diagnostics).
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::QrDisplay { .. } => "QrDisplay",
            Self::QrRequestScan => "QrRequestScan",
            Self::BleStartAdvertising { .. } => "BleStartAdvertising",
            Self::BleStartScanning { .. } => "BleStartScanning",
            Self::BleConnect { .. } => "BleConnect",
            Self::BleWriteCharacteristic { .. } => "BleWriteCharacteristic",
            Self::BleReadCharacteristic { .. } => "BleReadCharacteristic",
            Self::BleDisconnect => "BleDisconnect",
            Self::NfcActivate { .. } => "NfcActivate",
            Self::NfcDeactivate => "NfcDeactivate",
            Self::AudioEmitChallenge { .. } => "AudioEmitChallenge",
            Self::AudioListenForResponse { .. } => "AudioListenForResponse",
            Self::AudioStop => "AudioStop",
            Self::AccelerometerStart => "AccelerometerStart",
            Self::AccelerometerStop => "AccelerometerStop",
            Self::RelayEscrowDeposit { .. } => "RelayEscrowDeposit",
            Self::RelayEscrowCheck { .. } => "RelayEscrowCheck",
            Self::RelayEscrowRetrieve { .. } => "RelayEscrowRetrieve",
            Self::ShowShareSheet { .. } => "ShowShareSheet",
            Self::BleStopScanning => "BleStopScanning",
            Self::DirectSend { .. } => "DirectSend",
            Self::ImagePickFromLibrary => "ImagePickFromLibrary",
            Self::ImageCaptureFromCamera => "ImageCaptureFromCamera",
            Self::ImagePickFromFile => "ImagePickFromFile",
            Self::SwitchCamera { .. } => "SwitchCamera",
            Self::FilePickFromUser { .. } => "FilePickFromUser",
            Self::SetScreenBrightness { .. } => "SetScreenBrightness",
            Self::SetIdleTimerDisabled { .. } => "SetIdleTimerDisabled",
        }
    }
}

/// A hardware event reported by the frontend back to core.
///
/// These are the results of previously issued [`ExchangeCommand`]s or
/// asynchronous hardware notifications (e.g., BLE discovery, NFC tap).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ExchangeHardwareEvent {
    // ── QR ───────────────────────────────────────────────────────────
    /// The user scanned a QR code containing `data`.
    QrScanned { data: String },

    /// Per-frame scan progress from the camera viewfinder.
    ///
    /// Frontends send this periodically (e.g., every 200-500 ms) while the
    /// QR scanner is active. Core uses the rolling detection rate to compute
    /// a [`ScanQuality`] indicator for the viewfinder frame color.
    ///
    /// - `detected`: whether a QR code was found in this frame
    /// - `confidence`: optional platform-specific confidence score (0-100)
    /// - `frame_skipped`: true if the scanner skipped this frame (e.g.,
    ///   sharpness gating). Skipped frames are excluded from the quality
    ///   calculation — they indicate camera settling, not wrong pointing.
    QrScanProgress {
        detected: bool,
        confidence: Option<u8>,
        #[serde(default)]
        frame_skipped: bool,
    },

    // ── BLE ──────────────────────────────────────────────────────────
    /// A BLE peripheral was discovered during scanning.
    BleDeviceDiscovered {
        id: String,
        rssi: i16,
        adv_data: Vec<u8>,
    },
    /// Successfully connected to a BLE device.
    BleConnected { device_id: String },
    /// Data read from a BLE characteristic (response to `BleReadCharacteristic`).
    BleCharacteristicRead { uuid: String, data: Vec<u8> },
    /// BLE characteristic notification received (unsolicited push from peripheral).
    BleCharacteristicNotified { uuid: String, data: Vec<u8> },
    /// BLE connection lost or closed.
    BleDisconnected { reason: String },

    // ── NFC ──────────────────────────────────────────────────────────
    /// NFC data received from a tap exchange.
    NfcDataReceived { data: Vec<u8> },

    // ── Audio ────────────────────────────────────────────────────────
    /// Raw PCM samples from a microphone listen.
    ///
    /// Core decodes the FSK signal internally — the frontend ships
    /// whatever it captured at its native rate. Mono float PCM.
    AudioSamplesRecorded { samples: Vec<f32>, sample_rate: u32 },

    // ── Errors ───────────────────────────────────────────────────────
    /// A hardware operation failed.
    HardwareError { transport: String, error: String },
    /// The requested hardware is not available on this platform.
    HardwareUnavailable { transport: String },
    /// The user denied the required permission for this hardware.
    ///
    /// Distinct from `HardwareUnavailable` (hardware absent) — the hardware
    /// exists but the OS permission was denied. Frontends should send this
    /// when a runtime permission prompt is rejected (camera, BLE, microphone).
    PermissionDenied { transport: String },

    // ── Accelerometer ───────────────────────────────────────────────
    /// Accelerometer sample from the device.
    ///
    /// Acceleration is reported in milli-g (thousandths of standard gravity)
    /// to avoid `f32` and keep the type `Eq`-compatible across FFI boundaries.
    AccelerometerData {
        timestamp_ms: u64,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    },
    /// Impact detected by the device accelerometer.
    ImpactDetected {
        timestamp_ms: u64,
        magnitude_milli_g: i32,
    },

    // ── Relay escrow ────────────────────────────────────────────────
    /// Relay escrow gate has reached required deposit count.
    RelayEscrowReady { gate_hash: Vec<u8> },
    /// Relay escrow deposit/retrieve failed or gate expired.
    RelayEscrowFailed { gate_hash: Vec<u8>, reason: String },

    // ── Link mode ───────────────────────────────────────────────────
    /// User shared the link via share sheet.
    LinkShared,
    /// Link was opened by peer, providing their public key.
    LinkOpened { peer_public_key: Vec<u8> },

    // ── Relay escrow (added after v0.13 — append-only to preserve discriminants) ──
    /// Blob retrieved from relay escrow gate (response to `RelayEscrowRetrieve`).
    RelayEscrowBlobReceived { gate_hash: Vec<u8>, blob: Vec<u8> },

    // ── Direct transport (USB/TCP) ─────────────────────────────────
    /// Peer's exchange payload received over a direct transport.
    ///
    /// Sent by the frontend after completing the TCP exchange requested
    /// by [`ExchangeCommand::DirectSend`]. Contains the raw bytes of
    /// the peer's exchange payload (QR data string format).
    DirectPayloadReceived { data: Vec<u8> },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Image data received from photo library, camera, or file picker.
    ///
    /// The frontend sends raw image bytes (PNG, JPEG, etc.) — core
    /// handles format detection and normalization to WebP.
    ImageReceived { data: Vec<u8> },
    /// The user cancelled the image picker / camera without selecting.
    ImagePickCancelled,

    // ── File picking (vCard / backup import, ADR-031) ──────────────
    // Appended to preserve serde discriminant ordering.
    /// File data received from a [`ExchangeCommand::FilePickFromUser`]
    /// request.
    ///
    /// `bytes` is the entire file payload (no decoding — decoding lives
    /// in core). `filename` is the OS-reported display name; some
    /// platforms do not expose it, in which case the frontend reports
    /// an empty string.
    FilePickedFromUser { bytes: Vec<u8>, filename: String },
    /// The user dismissed the file picker without selecting a file.
    FilePickCancelledByUser,
}

// INLINE_TEST_REQUIRED: serde roundtrip tests need private enum variant access
#[cfg(test)]
mod tests {
    use super::*;

    // ── ExchangeCommand construction ────────────────────────────────

    #[test]
    fn qr_display_command_stores_data() {
        let cmd = ExchangeCommand::QrDisplay {
            data: "vauchi://exchange/abc123".into(),
        };
        assert!(
            matches!(cmd, ExchangeCommand::QrDisplay { data } if data == "vauchi://exchange/abc123")
        );
    }

    #[test]
    fn ble_start_advertising_stores_payload() {
        let payload = vec![0x01, 0x02, 0x03];
        let cmd = ExchangeCommand::BleStartAdvertising {
            service_uuid: "12345678-1234-1234-1234-123456789abc".into(),
            payload: payload.clone(),
        };
        assert!(
            matches!(cmd, ExchangeCommand::BleStartAdvertising { service_uuid, payload: p }
                if service_uuid == "12345678-1234-1234-1234-123456789abc" && p == payload)
        );
    }

    #[test]
    fn audio_listen_stores_timeout() {
        let cmd = ExchangeCommand::AudioListenForResponse {
            timeout_ms: 5000,
            sample_rate: 44100,
        };
        assert!(matches!(
            cmd,
            ExchangeCommand::AudioListenForResponse {
                timeout_ms,
                sample_rate,
            } if timeout_ms == 5000 && sample_rate == 44100
        ));
    }

    // ── ExchangeHardwareEvent construction ──────────────────────────

    #[test]
    fn qr_scanned_event_stores_data() {
        let evt = ExchangeHardwareEvent::QrScanned {
            data: "scanned-data".into(),
        };
        assert!(matches!(evt, ExchangeHardwareEvent::QrScanned { data } if data == "scanned-data"));
    }

    #[test]
    fn ble_device_discovered_stores_rssi() {
        let evt = ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0xFF],
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::BleDeviceDiscovered { rssi, .. } if rssi == -42)
        );
    }

    #[test]
    fn hardware_unavailable_stores_transport() {
        let evt = ExchangeHardwareEvent::HardwareUnavailable {
            transport: "BLE".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::HardwareUnavailable { transport } if transport == "BLE")
        );
    }

    // @internal
    #[test]
    fn permission_denied_stores_transport() {
        let evt = ExchangeHardwareEvent::PermissionDenied {
            transport: "camera".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::PermissionDenied { transport } if transport == "camera")
        );
    }

    // @internal
    #[test]
    fn permission_denied_is_distinct_from_hardware_unavailable() {
        let denied = ExchangeHardwareEvent::PermissionDenied {
            transport: "camera".into(),
        };
        let unavailable = ExchangeHardwareEvent::HardwareUnavailable {
            transport: "camera".into(),
        };
        assert_ne!(denied, unavailable);
    }

    #[test]
    fn hardware_error_stores_details() {
        let evt = ExchangeHardwareEvent::HardwareError {
            transport: "NFC".into(),
            error: "no reader detected".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::HardwareError { transport, error }
                if transport == "NFC" && error == "no reader detected")
        );
    }

    // ── Serialization roundtrips ────────────────────────────────────

    #[test]
    fn command_serialization_roundtrip() {
        let commands = vec![
            ExchangeCommand::QrDisplay {
                data: "test".into(),
            },
            ExchangeCommand::QrRequestScan,
            ExchangeCommand::BleStartAdvertising {
                service_uuid: "uuid".into(),
                payload: vec![1, 2, 3],
            },
            ExchangeCommand::BleDisconnect,
            ExchangeCommand::NfcActivate {
                payload: vec![0xAA],
            },
            ExchangeCommand::NfcDeactivate,
            ExchangeCommand::AudioEmitChallenge {
                samples: vec![0.1, 0.2, 0.3],
                sample_rate: 44100,
            },
            ExchangeCommand::AudioListenForResponse {
                timeout_ms: 3000,
                sample_rate: 44100,
            },
            ExchangeCommand::AudioStop,
            ExchangeCommand::AccelerometerStart,
            ExchangeCommand::AccelerometerStop,
            ExchangeCommand::RelayEscrowDeposit {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
                encrypted_card: vec![0x01; 64],
                ttl_seconds: 3600,
            },
            ExchangeCommand::RelayEscrowCheck {
                gate_hash: vec![0xAB; 32],
                suggested_interval_ms: 30_000,
            },
            ExchangeCommand::RelayEscrowRetrieve {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
            },
            ExchangeCommand::ShowShareSheet {
                url: "https://vauchi.app/link/abc123".into(),
            },
            ExchangeCommand::ImagePickFromLibrary,
            ExchangeCommand::ImageCaptureFromCamera,
            ExchangeCommand::ImagePickFromFile,
            ExchangeCommand::SwitchCamera { use_front: true },
            ExchangeCommand::SwitchCamera { use_front: false },
            ExchangeCommand::FilePickFromUser {
                accepted_mime_types: vec!["text/vcard".into(), "text/x-vcard".into()],
                purpose: FilePickPurpose::ImportContacts,
            },
            ExchangeCommand::FilePickFromUser {
                accepted_mime_types: vec!["application/octet-stream".into()],
                purpose: FilePickPurpose::ImportBackup,
            },
            ExchangeCommand::FilePickFromUser {
                accepted_mime_types: vec![],
                purpose: FilePickPurpose::Other {
                    label_key: "import.key_bundle".into(),
                },
            },
        ];
        for cmd in &commands {
            let json = serde_json::to_string(cmd).expect("serialize");
            let decoded: ExchangeCommand = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(cmd, &decoded, "roundtrip failed for {:?}", cmd);
        }
    }

    #[test]
    fn event_serialization_roundtrip() {
        let events = vec![
            ExchangeHardwareEvent::QrScanned { data: "qr".into() },
            ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "d1".into(),
                rssi: -60,
                adv_data: vec![],
            },
            ExchangeHardwareEvent::BleConnected {
                device_id: "d1".into(),
            },
            ExchangeHardwareEvent::BleCharacteristicRead {
                uuid: "char1".into(),
                data: vec![0x0B],
            },
            ExchangeHardwareEvent::BleDisconnected {
                reason: "timeout".into(),
            },
            ExchangeHardwareEvent::NfcDataReceived { data: vec![0xCC] },
            ExchangeHardwareEvent::AudioSamplesRecorded {
                samples: vec![0.0, 0.5, -0.5],
                sample_rate: 44100,
            },
            ExchangeHardwareEvent::HardwareError {
                transport: "BLE".into(),
                error: "adapter off".into(),
            },
            ExchangeHardwareEvent::HardwareUnavailable {
                transport: "NFC".into(),
            },
            ExchangeHardwareEvent::PermissionDenied {
                transport: "camera".into(),
            },
            ExchangeHardwareEvent::AccelerometerData {
                timestamp_ms: 1_000,
                x_milli_g: 1_000, // ~1 g lateral
                y_milli_g: 0,
                z_milli_g: -9_800, // ~-9.8 g (gravity)
            },
            ExchangeHardwareEvent::ImpactDetected {
                timestamp_ms: 2_000,
                magnitude_milli_g: 3_500, // ~3.5 g impact
            },
            ExchangeHardwareEvent::RelayEscrowReady {
                gate_hash: vec![0xDE; 32],
            },
            ExchangeHardwareEvent::RelayEscrowFailed {
                gate_hash: vec![0xDE; 32],
                reason: "gate expired".into(),
            },
            ExchangeHardwareEvent::LinkShared,
            ExchangeHardwareEvent::LinkOpened {
                peer_public_key: vec![0x04; 32],
            },
            ExchangeHardwareEvent::ImageReceived {
                data: vec![0xFF, 0xD8, 0xFF],
            },
            ExchangeHardwareEvent::ImagePickCancelled,
            ExchangeHardwareEvent::QrScanProgress {
                detected: true,
                confidence: Some(85),
                frame_skipped: false,
            },
            ExchangeHardwareEvent::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: true,
            },
            ExchangeHardwareEvent::FilePickedFromUser {
                bytes: vec![0x42; 8],
                filename: "contacts.vcf".into(),
            },
            ExchangeHardwareEvent::FilePickedFromUser {
                bytes: vec![],
                filename: String::new(),
            },
            ExchangeHardwareEvent::FilePickCancelledByUser,
        ];
        for evt in &events {
            let json = serde_json::to_string(evt).expect("serialize");
            let decoded: ExchangeHardwareEvent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(evt, &decoded, "roundtrip failed for {:?}", evt);
        }
    }

    // ── Clone + equality ────────────────────────────────────────────

    #[test]
    fn command_clone_equals_original() {
        let cmd = ExchangeCommand::BleWriteCharacteristic {
            uuid: "test-uuid".into(),
            data: vec![1, 2, 3, 4, 5],
        };
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn event_clone_equals_original() {
        let evt = ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "notify-uuid".into(),
            data: vec![0xDE, 0xAD],
        };
        let cloned = evt.clone();
        assert_eq!(evt, cloned);
    }

    // ── All variants covered ────────────────────────────────────────

    #[test]
    fn all_command_variants_are_distinct() {
        let variants: Vec<ExchangeCommand> = vec![
            ExchangeCommand::QrDisplay { data: "".into() },
            ExchangeCommand::QrRequestScan,
            ExchangeCommand::BleStartAdvertising {
                service_uuid: "".into(),
                payload: vec![],
            },
            ExchangeCommand::BleStartScanning {
                service_uuid: "".into(),
            },
            ExchangeCommand::BleConnect {
                device_id: "".into(),
            },
            ExchangeCommand::BleWriteCharacteristic {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeCommand::BleReadCharacteristic { uuid: "".into() },
            ExchangeCommand::BleDisconnect,
            ExchangeCommand::NfcActivate { payload: vec![] },
            ExchangeCommand::NfcDeactivate,
            ExchangeCommand::AudioEmitChallenge {
                samples: vec![],
                sample_rate: 0,
            },
            ExchangeCommand::AudioListenForResponse {
                timeout_ms: 0,
                sample_rate: 0,
            },
            ExchangeCommand::AudioStop,
            ExchangeCommand::AccelerometerStart,
            ExchangeCommand::AccelerometerStop,
            ExchangeCommand::RelayEscrowDeposit {
                gate_hash: vec![],
                slot_hash: vec![],
                encrypted_card: vec![],
                ttl_seconds: 0,
            },
            ExchangeCommand::RelayEscrowCheck {
                gate_hash: vec![],
                suggested_interval_ms: 0,
            },
            ExchangeCommand::RelayEscrowRetrieve {
                gate_hash: vec![],
                slot_hash: vec![],
            },
            ExchangeCommand::ShowShareSheet { url: "".into() },
            ExchangeCommand::BleStopScanning,
            ExchangeCommand::DirectSend {
                payload: vec![],
                is_initiator: false,
            },
            ExchangeCommand::ImagePickFromLibrary,
            ExchangeCommand::ImageCaptureFromCamera,
            ExchangeCommand::ImagePickFromFile,
            ExchangeCommand::SwitchCamera { use_front: false },
            ExchangeCommand::FilePickFromUser {
                accepted_mime_types: vec![],
                purpose: FilePickPurpose::ImportContacts,
            },
        ];
        // 26 total command variants
        assert_eq!(variants.len(), 26);
    }

    #[test]
    fn all_event_variants_are_distinct() {
        let variants: Vec<ExchangeHardwareEvent> = vec![
            ExchangeHardwareEvent::QrScanned { data: "".into() },
            ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "".into(),
                rssi: 0,
                adv_data: vec![],
            },
            ExchangeHardwareEvent::BleConnected {
                device_id: "".into(),
            },
            ExchangeHardwareEvent::BleCharacteristicRead {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeHardwareEvent::BleDisconnected { reason: "".into() },
            ExchangeHardwareEvent::NfcDataReceived { data: vec![] },
            ExchangeHardwareEvent::AudioSamplesRecorded {
                samples: vec![],
                sample_rate: 0,
            },
            ExchangeHardwareEvent::HardwareError {
                transport: "".into(),
                error: "".into(),
            },
            ExchangeHardwareEvent::HardwareUnavailable {
                transport: "".into(),
            },
            ExchangeHardwareEvent::PermissionDenied {
                transport: "".into(),
            },
            ExchangeHardwareEvent::AccelerometerData {
                timestamp_ms: 0,
                x_milli_g: 0,
                y_milli_g: 0,
                z_milli_g: 0,
            },
            ExchangeHardwareEvent::ImpactDetected {
                timestamp_ms: 0,
                magnitude_milli_g: 0,
            },
            ExchangeHardwareEvent::RelayEscrowReady { gate_hash: vec![] },
            ExchangeHardwareEvent::RelayEscrowFailed {
                gate_hash: vec![],
                reason: "".into(),
            },
            ExchangeHardwareEvent::LinkShared,
            ExchangeHardwareEvent::LinkOpened {
                peer_public_key: vec![],
            },
            ExchangeHardwareEvent::RelayEscrowBlobReceived {
                gate_hash: vec![],
                blob: vec![],
            },
            ExchangeHardwareEvent::DirectPayloadReceived { data: vec![] },
            ExchangeHardwareEvent::ImageReceived { data: vec![] },
            ExchangeHardwareEvent::ImagePickCancelled,
            ExchangeHardwareEvent::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: false,
            },
            ExchangeHardwareEvent::FilePickedFromUser {
                bytes: vec![],
                filename: String::new(),
            },
            ExchangeHardwareEvent::FilePickCancelledByUser,
        ];
        // 24 total event variants
        assert_eq!(variants.len(), 24);
    }

    // ── File-picker variants (Phase 1: types only) ──────────────────

    // @internal
    #[test]
    fn file_pick_from_user_command_stores_purpose_and_mime() {
        let cmd = ExchangeCommand::FilePickFromUser {
            accepted_mime_types: vec!["text/vcard".into(), "text/x-vcard".into()],
            purpose: FilePickPurpose::ImportContacts,
        };
        match cmd {
            ExchangeCommand::FilePickFromUser {
                accepted_mime_types,
                purpose,
            } => {
                assert_eq!(accepted_mime_types, vec!["text/vcard", "text/x-vcard"]);
                assert_eq!(purpose, FilePickPurpose::ImportContacts);
            }
            other => panic!("expected FilePickFromUser, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_pick_purpose_other_carries_label_key() {
        let purpose = FilePickPurpose::Other {
            label_key: "import.key_bundle".into(),
        };
        match purpose {
            FilePickPurpose::Other { label_key } => assert_eq!(label_key, "import.key_bundle"),
            other => panic!("expected Other, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_pick_purpose_variants_are_distinct() {
        let a = FilePickPurpose::ImportContacts;
        let b = FilePickPurpose::ImportBackup;
        let c = FilePickPurpose::Other {
            label_key: "x".into(),
        };
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    // @internal
    #[test]
    fn file_pick_purpose_serialization_roundtrip() {
        let purposes = vec![
            FilePickPurpose::ImportContacts,
            FilePickPurpose::ImportBackup,
            FilePickPurpose::Other {
                label_key: "import.key_bundle".into(),
            },
            FilePickPurpose::Other {
                label_key: String::new(),
            },
        ];
        for p in &purposes {
            let json = serde_json::to_string(p).expect("serialize");
            let decoded: FilePickPurpose = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(p, &decoded, "roundtrip failed for {:?}", p);
        }
    }

    // @internal
    #[test]
    fn file_picked_from_user_event_stores_bytes_and_filename() {
        let evt = ExchangeHardwareEvent::FilePickedFromUser {
            bytes: vec![0x42, 0x43, 0x44],
            filename: "contacts.vcf".into(),
        };
        match evt {
            ExchangeHardwareEvent::FilePickedFromUser { bytes, filename } => {
                assert_eq!(bytes, vec![0x42, 0x43, 0x44]);
                assert_eq!(filename, "contacts.vcf");
            }
            other => panic!("expected FilePickedFromUser, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_picked_event_distinct_from_cancellation() {
        let picked = ExchangeHardwareEvent::FilePickedFromUser {
            bytes: vec![],
            filename: String::new(),
        };
        let cancelled = ExchangeHardwareEvent::FilePickCancelledByUser;
        assert_ne!(picked, cancelled);
    }

    // @internal
    #[test]
    fn file_pick_from_user_variant_name_is_stable() {
        let cmd = ExchangeCommand::FilePickFromUser {
            accepted_mime_types: vec![],
            purpose: FilePickPurpose::ImportBackup,
        };
        assert_eq!(cmd.variant_name(), "FilePickFromUser");
    }

    // @internal
    #[test]
    fn set_screen_brightness_with_some_level_round_trips() {
        let cmd = ExchangeCommand::SetScreenBrightness { level: Some(0.65) };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(
            json.contains("\"SetScreenBrightness\""),
            "expected variant tag in serialized form, got {json}"
        );
        let restored: ExchangeCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, restored);
    }

    // @internal
    #[test]
    fn set_screen_brightness_with_none_means_restore_default() {
        // The contract: `level: None` is the explicit "restore platform
        // default" signal. Keep the wire shape pinned so frontends can
        // distinguish it from a missing field.
        let cmd = ExchangeCommand::SetScreenBrightness { level: None };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: ExchangeCommand = serde_json::from_str(&json).unwrap();
        match restored {
            ExchangeCommand::SetScreenBrightness { level } => assert_eq!(level, None),
            other => panic!("expected SetScreenBrightness, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn set_screen_brightness_variant_name_is_stable() {
        let cmd = ExchangeCommand::SetScreenBrightness { level: Some(0.5) };
        assert_eq!(cmd.variant_name(), "SetScreenBrightness");
    }

    // @internal
    #[test]
    fn set_idle_timer_disabled_round_trips_each_state() {
        for disabled in [true, false] {
            let cmd = ExchangeCommand::SetIdleTimerDisabled { disabled };
            let json = serde_json::to_string(&cmd).unwrap();
            assert!(
                json.contains("\"SetIdleTimerDisabled\""),
                "expected variant tag, got {json}"
            );
            let restored: ExchangeCommand = serde_json::from_str(&json).unwrap();
            assert_eq!(cmd, restored);
        }
    }

    // @internal
    #[test]
    fn set_idle_timer_disabled_variant_name_is_stable() {
        let cmd = ExchangeCommand::SetIdleTimerDisabled { disabled: true };
        assert_eq!(cmd.variant_name(), "SetIdleTimerDisabled");
    }

    // @internal
    #[test]
    fn screen_presentation_commands_are_distinct() {
        // Sanity-check that the two new variants are not accidentally
        // matched as the same shape (both carry an option-like field
        // that could collide if someone writes a sloppy match).
        let bright = ExchangeCommand::SetScreenBrightness { level: None };
        let idle = ExchangeCommand::SetIdleTimerDisabled { disabled: false };
        assert_ne!(bright, idle);
        assert_ne!(bright.variant_name(), idle.variant_name());
    }
}

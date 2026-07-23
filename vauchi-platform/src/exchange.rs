// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange UniFFI mirror types.
//!
//! Slice 32m retired the `MobileExchangeSession` cycle-thread wrapper and
//! its proximity bridges; the AppEngine command/event machine
//! (`vauchi-app`) now drives every exchange flow. This module keeps only
//! the UniFFI-facing mirror types that survive: the `MobileExchangeState`
//! view enum (consumed by `exchange_view`) and the `MobileCommand` /
//! `MobileEvent` mirrors of `vauchi_core::Command` / `Event` (ADR-031).

use vauchi_core::{Command, Event};

/// Mobile-friendly exchange state (no raw bytes or core types).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileExchangeState {
    Idle,
    DisplayingQr {
        qr_data: String,
    },
    PeerScanned,
    AwaitingKeyAgreement,
    AwaitingCardExchange,
    Complete {
        contact_id: String,
        contact_name: String,
    },
    Failed {
        error: String,
    },
}

// === ADR-031: Command/Event UniFFI Exports ===

/// Exchange command sent from core to the frontend (ADR-031).
///
/// Mobile apps match on these and dispatch to platform-specific APIs
/// (camera, BLE stack, NFC reader, audio subsystem).
#[derive(uniffi::Enum, Debug, Clone)]
pub enum MobileCommand {
    // QR
    QrDisplay {
        data: String,
    },
    QrRequestScan,
    // BLE
    BleStartAdvertising {
        service_uuid: String,
        payload: Vec<u8>,
    },
    BleStartScanning {
        service_uuid: String,
    },
    BleStopScanning,
    BleConnect {
        device_id: String,
    },
    BleWriteCharacteristic {
        device_id: String,
        uuid: String,
        data: Vec<u8>,
    },
    BleReadCharacteristic {
        device_id: String,
        uuid: String,
    },
    /// Disconnect one specific link (`device_id` + `direction` name it), so
    /// glare resolution can drop the losing link while the survivor keeps
    /// carrying the handshake. An empty `device_id` means "the current link"
    /// (pre-connect teardown).
    BleDisconnect {
        device_id: String,
        direction: MobileBleLinkDirection,
    },
    // NFC
    NfcActivate {
        payload: Vec<u8>,
    },
    NfcDeactivate,
    NfcSendApdu {
        data: Vec<u8>,
    },
    // Audio (PCM samples — core encodes the FSK challenge before sending)
    AudioEmitChallenge {
        samples: Vec<f32>,
        sample_rate: u32,
    },
    AudioListenForResponse {
        timeout_ms: u64,
        sample_rate: u32,
    },
    AudioStop,
    // Accelerometer
    AccelerometerStart,
    AccelerometerStop,
    // Relay escrow
    RelayEscrowDeposit {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
        encrypted_card: Vec<u8>,
        ttl_seconds: u32,
    },
    RelayEscrowCheck {
        gate_hash: Vec<u8>,
        suggested_interval_ms: u32,
    },
    RelayEscrowRetrieve {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
    },
    // Link mode
    ShowShareSheet {
        url: String,
    },
    // Direct transport (USB cable)
    DirectSend {
        payload: Vec<u8>,
        is_initiator: bool,
    },
    // USB/direct-transport card-exchange second leg: send our encrypted card.
    DirectSendCard {
        ciphertext: Vec<u8>,
        is_initiator: bool,
    },
}

impl From<Command> for MobileCommand {
    fn from(cmd: Command) -> Self {
        match cmd {
            Command::QrDisplay { data } => Self::QrDisplay { data },
            Command::QrRequestScan => Self::QrRequestScan,
            Command::BleStartAdvertising {
                service_uuid,
                payload,
            } => Self::BleStartAdvertising {
                service_uuid,
                payload,
            },
            Command::BleStartScanning { service_uuid } => Self::BleStartScanning { service_uuid },
            Command::BleStopScanning => Self::BleStopScanning,
            Command::BleConnect { device_id } => Self::BleConnect { device_id },
            Command::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            } => Self::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            },
            Command::BleReadCharacteristic { device_id, uuid } => {
                Self::BleReadCharacteristic { device_id, uuid }
            }
            Command::BleDisconnect {
                device_id,
                direction,
            } => Self::BleDisconnect {
                device_id,
                direction: direction.into(),
            },
            Command::NfcActivate { payload } => Self::NfcActivate { payload },
            Command::NfcDeactivate => Self::NfcDeactivate,
            Command::NfcSendApdu { data } => Self::NfcSendApdu { data },
            Command::AudioEmitChallenge {
                samples,
                sample_rate,
            } => Self::AudioEmitChallenge {
                samples,
                sample_rate,
            },
            Command::AudioListenForResponse {
                timeout_ms,
                sample_rate,
            } => Self::AudioListenForResponse {
                timeout_ms,
                sample_rate,
            },
            Command::AudioStop => Self::AudioStop,
            Command::AccelerometerStart => Self::AccelerometerStart,
            Command::AccelerometerStop => Self::AccelerometerStop,
            Command::RelayEscrowDeposit {
                gate_hash,
                slot_hash,
                encrypted_card,
                ttl_seconds,
            } => Self::RelayEscrowDeposit {
                gate_hash,
                slot_hash,
                encrypted_card,
                ttl_seconds,
            },
            Command::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms,
            } => Self::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms,
            },
            Command::RelayEscrowRetrieve {
                gate_hash,
                slot_hash,
            } => Self::RelayEscrowRetrieve {
                gate_hash,
                slot_hash,
            },
            Command::ShowShareSheet { url } => Self::ShowShareSheet { url },
            Command::DirectSend {
                payload,
                is_initiator,
            } => Self::DirectSend {
                payload,
                is_initiator,
            },
            Command::DirectSendCard {
                ciphertext,
                is_initiator,
            } => Self::DirectSendCard {
                ciphertext,
                is_initiator,
            },
            _ => Self::QrRequestScan,
        }
    }
}

/// Hardware event reported by the frontend back to core (ADR-031).
///
/// Mobile apps create these after executing a command (e.g., QR scanned,
/// BLE data received) and feed them back via `apply_hardware_event()`.
/// Physical GATT link direction reported on [`MobileEvent::BleConnected`].
/// Mirrors [`vauchi_core::platform::BleLinkDirection`]; the shell reports
/// `Outbound` when it is the central (it dialed out) and `Inbound` when it is
/// the peripheral (a peer connected to it). Core derives the handshake role
/// from this, not from the token tiebreak.
#[derive(uniffi::Enum, Debug, Clone)]
pub enum MobileBleLinkDirection {
    Outbound,
    Inbound,
}

impl From<MobileBleLinkDirection> for vauchi_core::platform::BleLinkDirection {
    fn from(d: MobileBleLinkDirection) -> Self {
        match d {
            MobileBleLinkDirection::Outbound => Self::Outbound,
            MobileBleLinkDirection::Inbound => Self::Inbound,
        }
    }
}

impl From<vauchi_core::platform::BleLinkDirection> for MobileBleLinkDirection {
    fn from(d: vauchi_core::platform::BleLinkDirection) -> Self {
        match d {
            vauchi_core::platform::BleLinkDirection::Outbound => Self::Outbound,
            vauchi_core::platform::BleLinkDirection::Inbound => Self::Inbound,
            // `#[non_exhaustive]` forward compat: an unknown future direction
            // maps to Outbound — the shell drops the link it dialed, never a
            // link it didn't know it had.
            _ => Self::Outbound,
        }
    }
}

#[derive(uniffi::Enum, Debug, Clone)]
pub enum MobileEvent {
    // QR
    QrScanned {
        data: String,
    },
    // BLE
    BleDeviceDiscovered {
        id: String,
        rssi: i16,
        adv_data: Vec<u8>,
    },
    BleConnected {
        device_id: String,
        direction: MobileBleLinkDirection,
    },
    BleCharacteristicRead {
        device_id: String,
        uuid: String,
        data: Vec<u8>,
    },
    BleCharacteristicNotified {
        device_id: String,
        uuid: String,
        data: Vec<u8>,
    },
    BleDisconnected {
        device_id: String,
        direction: MobileBleLinkDirection,
        reason: String,
    },
    // NFC
    NfcDataReceived {
        data: Vec<u8>,
    },
    // Audio (raw PCM — core decodes the FSK signal internally)
    AudioSamplesRecorded {
        samples: Vec<f32>,
        sample_rate: u32,
    },
    // Accelerometer
    AccelerometerData {
        timestamp_ms: u64,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    },
    ImpactDetected {
        timestamp_ms: u64,
        magnitude_milli_g: i32,
    },
    // Relay escrow
    RelayEscrowReady {
        gate_hash: Vec<u8>,
    },
    RelayEscrowBlobReceived {
        gate_hash: Vec<u8>,
        blob: Vec<u8>,
    },
    RelayEscrowFailed {
        gate_hash: Vec<u8>,
        reason: String,
    },
    // Link mode
    LinkShared,
    LinkOpened {
        peer_public_key: Vec<u8>,
    },
    // Direct transport (USB cable)
    DirectPayloadReceived {
        data: Vec<u8>,
    },
    // USB/direct-transport card-exchange second leg: the peer's encrypted card.
    DirectCardReceived {
        ciphertext: Vec<u8>,
    },
    // Image (avatar picker / camera)
    ImageReceived {
        data: Vec<u8>,
    },
    ImagePickCancelled,
    // File picking (vCard / backup import, ADR-031)
    FilePickedFromUser {
        bytes: Vec<u8>,
        filename: String,
    },
    FilePickCancelledByUser,
    // Biometric auth (ADR-031)
    BiometricUnlockSucceeded,
    // Errors
    HardwareError {
        transport: String,
        error: String,
    },
    HardwareUnavailable {
        transport: String,
    },
    PermissionDenied {
        transport: String,
    },
    /// Device location fix in reply to `Command::LocationRequest` (ADR-051
    /// capture-at-exchange). Coordinates are decimal degrees; `accuracy_meters`
    /// is the provider's reported horizontal accuracy, if any. A declined
    /// permission / absent provider is reported via the generic
    /// `PermissionDenied { transport: "location" }` / `HardwareUnavailable`.
    LocationResult {
        latitude: f64,
        longitude: f64,
        accuracy_meters: Option<f32>,
    },
}

impl From<MobileEvent> for Event {
    fn from(evt: MobileEvent) -> Self {
        match evt {
            MobileEvent::QrScanned { data } => Self::QrScanned { data },
            MobileEvent::BleDeviceDiscovered { id, rssi, adv_data } => {
                Self::BleDeviceDiscovered { id, rssi, adv_data }
            }
            MobileEvent::BleConnected {
                device_id,
                direction,
            } => Self::BleConnected {
                device_id,
                direction: direction.into(),
            },
            MobileEvent::BleCharacteristicRead {
                device_id,
                uuid,
                data,
            } => Self::BleCharacteristicRead {
                device_id,
                uuid,
                data,
            },
            MobileEvent::BleCharacteristicNotified {
                device_id,
                uuid,
                data,
            } => Self::BleCharacteristicNotified {
                device_id,
                uuid,
                data,
            },
            MobileEvent::BleDisconnected {
                device_id,
                direction,
                reason,
            } => Self::BleDisconnected {
                device_id,
                direction: direction.into(),
                reason,
            },
            MobileEvent::NfcDataReceived { data } => Self::NfcDataReceived { data },
            MobileEvent::AudioSamplesRecorded {
                samples,
                sample_rate,
            } => Self::AudioSamplesRecorded {
                samples,
                sample_rate,
            },
            MobileEvent::HardwareError { transport, error } => {
                Self::HardwareError { transport, error }
            }
            MobileEvent::HardwareUnavailable { transport } => {
                Self::HardwareUnavailable { transport }
            }
            MobileEvent::PermissionDenied { transport } => Self::PermissionDenied { transport },
            MobileEvent::LocationResult {
                latitude,
                longitude,
                accuracy_meters,
            } => Self::LocationResult {
                latitude,
                longitude,
                accuracy_meters,
            },
            MobileEvent::AccelerometerData {
                timestamp_ms,
                x_milli_g,
                y_milli_g,
                z_milli_g,
            } => Self::AccelerometerData {
                timestamp_ms,
                x_milli_g,
                y_milli_g,
                z_milli_g,
            },
            MobileEvent::ImpactDetected {
                timestamp_ms,
                magnitude_milli_g,
            } => Self::ImpactDetected {
                timestamp_ms,
                magnitude_milli_g,
            },
            MobileEvent::RelayEscrowReady { gate_hash } => Self::RelayEscrowReady { gate_hash },
            MobileEvent::RelayEscrowBlobReceived { gate_hash, blob } => {
                Self::RelayEscrowBlobReceived { gate_hash, blob }
            }
            MobileEvent::RelayEscrowFailed { gate_hash, reason } => {
                Self::RelayEscrowFailed { gate_hash, reason }
            }
            MobileEvent::LinkShared => Self::LinkShared,
            MobileEvent::LinkOpened { peer_public_key } => Self::LinkOpened { peer_public_key },
            MobileEvent::DirectPayloadReceived { data } => Self::DirectPayloadReceived { data },
            MobileEvent::DirectCardReceived { ciphertext } => {
                Self::DirectCardReceived { ciphertext }
            }
            MobileEvent::ImageReceived { data } => Self::ImageReceived { data },
            MobileEvent::ImagePickCancelled => Self::ImagePickCancelled,
            MobileEvent::FilePickedFromUser { bytes, filename } => {
                Self::FilePickedFromUser { bytes, filename }
            }
            MobileEvent::FilePickCancelledByUser => Self::FilePickCancelledByUser,
            MobileEvent::BiometricUnlockSucceeded => Self::BiometricUnlockSucceeded,
        }
    }
}

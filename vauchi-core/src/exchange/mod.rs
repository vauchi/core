// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

#[cfg(feature = "testing")]
pub mod accelerometer;
#[cfg(not(feature = "testing"))]
mod accelerometer;

#[cfg(feature = "testing")]
pub mod ambient_audio;
#[cfg(not(feature = "testing"))]
mod ambient_audio;

#[cfg(feature = "testing")]
pub mod audio;
#[cfg(not(feature = "testing"))]
mod audio;

#[cfg(feature = "audio-cpal")]
pub mod audio_cpal;

#[cfg(feature = "testing")]
#[allow(deprecated)]
pub mod ble;
#[cfg(not(feature = "testing"))]
#[allow(deprecated)]
mod ble;

#[cfg(feature = "testing")]
pub mod ble_chunking;
#[cfg(not(feature = "testing"))]
mod ble_chunking;

#[cfg(feature = "testing")]
pub mod ble_handshake;
#[cfg(not(feature = "testing"))]
mod ble_handshake;

#[cfg(feature = "testing")]
pub mod ble_payload;
#[cfg(not(feature = "testing"))]
mod ble_payload;

#[cfg(feature = "testing")]
pub mod ble_rollback;
#[cfg(not(feature = "testing"))]
mod ble_rollback;

#[cfg(feature = "testing")]
pub mod multistage;
#[cfg(not(feature = "testing"))]
mod multistage;

pub mod capability;
pub mod card_snapshot;
pub mod command;
pub mod device_link;
pub mod exchange_id;
pub mod exchange_record;
pub mod mode;
pub mod mode_availability;
pub mod mode_payload;
pub mod persisted_state;

#[allow(deprecated)]
pub mod transport;

#[cfg(feature = "testing")]
pub mod verifier_chain;
#[cfg(not(feature = "testing"))]
mod verifier_chain;

#[cfg(any(test, feature = "testing"))]
pub mod verifier_harness;

#[cfg(feature = "testing")]
pub mod verifier_event;
#[cfg(not(feature = "testing"))]
mod verifier_event;

#[cfg(feature = "testing")]
pub mod trust_metrics;
#[cfg(not(feature = "testing"))]
mod trust_metrics;

pub mod tcp_transport;

#[cfg(feature = "testing")]
pub mod exchange_payload;
#[cfg(not(feature = "testing"))]
mod exchange_payload;

#[cfg(feature = "testing")]
pub mod nfc_active;
#[cfg(not(feature = "testing"))]
mod nfc_active;

#[cfg(feature = "testing")]
pub mod nfc_apdu_chaining;
#[cfg(not(feature = "testing"))]
mod nfc_apdu_chaining;

#[cfg(feature = "testing")]
pub mod nfc_card_payload;
#[cfg(not(feature = "testing"))]
mod nfc_card_payload;

#[cfg(feature = "testing")]
pub mod nfc_handshake;
#[cfg(not(feature = "testing"))]
mod nfc_handshake;

#[cfg(feature = "testing")]
pub mod nfc_rollback;
#[cfg(not(feature = "testing"))]
mod nfc_rollback;

#[cfg(feature = "testing")]
pub mod encrypted_message;
#[cfg(not(feature = "testing"))]
mod encrypted_message;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod proximity;
#[cfg(not(feature = "testing"))]
mod proximity;

#[cfg(feature = "testing")]
pub mod qr;
#[cfg(not(feature = "testing"))]
mod qr;

#[cfg(feature = "testing")]
pub mod session;
#[cfg(not(feature = "testing"))]
mod session;

#[cfg(feature = "testing")]
pub mod x3dh;
#[cfg(not(feature = "testing"))]
mod x3dh;

pub use accelerometer::{
    AccelerometerBackend, AccelerometerConfig, AccelerometerSample, AccelerometerVerifier,
    MockAccelerometerBackend,
};
pub use ambient_audio::{
    AmbientAudioBackend, AmbientAudioConfig, AmbientAudioVerifier, AudioFingerprint,
    MockAmbientAudioBackend,
};
pub use audio::{AudioBackend, AudioCapability, AudioConfig, MockAudioBackend, UltrasonicVerifier};
#[cfg(feature = "audio-cpal")]
pub use audio_cpal::CpalAudioBackend;
#[allow(deprecated)]
pub use ble::{
    BLE_DEFAULT_USABLE, BLE_MIN_MTU, BLE_PAYLOAD_SIZE, BLEAdvertisement, BLEDevice, BLEError,
    BLEExchangeSession, BLEExchangeState, BLEProximityVerifier, BLETransport, CHAR_CARD_EXCHANGE,
    CHAR_CHALLENGE, CHAR_DATA_NOTIFY, CHAR_DATA_WRITE, CHAR_EXCHANGE_PAYLOAD,
    CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE, ExchangeBle, MockBLETransport, MockBLEVerifier,
    VAUCHI_BLE_DIAGNOSTIC_SERVICE_UUID, VAUCHI_BLE_SERVICE_UUID,
};
pub use ble_chunking::{BLE_CHUNK_OVERHEAD, BleChunker, BleReassembler};
pub use ble_handshake::{
    BLE_HANDSHAKE_INFO, BLE_HANDSHAKE_VERSION, BleExchangeResult, BleHandshakeSession,
    BleHandshakeState,
};
pub use ble_payload::BleCardPayload;
pub use ble_rollback::BleRollback;
pub use command::{ExchangeCommand, ExchangeHardwareEvent};
pub use device_link::{
    DeviceLinkConfirmation, DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR,
    DeviceLinkRequest, DeviceLinkResponder, DeviceLinkResponse, ProximityProof,
    compute_confirmation_mac, generate_numeric_code,
};
pub use encrypted_message::{DecryptedExchangePayload, EncryptedExchangeMessage};
pub use error::ExchangeError;
pub use nfc_active::apdu as nfc_apdu;
pub use nfc_active::{ExchangeNfc, NFC_PAYLOAD_SIZE};
pub use nfc_apdu_chaining::{
    MAX_APDU_DATA, extract_data, is_chained, reassemble_chain, split_into_chain,
};
pub use nfc_card_payload::NfcCardPayload;
pub use nfc_handshake::{NfcExchangeResult, NfcHandshakeSession, NfcHandshakeState};
pub use nfc_rollback::{NfcRollback, NoopNfcRollback};
#[cfg(any(test, feature = "testing"))]
pub use proximity::MockProximityVerifier;
pub use proximity::{
    ManualConfirmationVerifier, ProximityConfidence, ProximityError, ProximityVerifier,
};
pub use qr::{ExchangeQR, check_clock_drift};
pub use session::{
    DefaultPlatformCallbacks, DuplicateAction, ExchangeEvent, ExchangePlatformCallbacks,
    ExchangeSession, ExchangeState, ExchangeTransport,
};
pub use trust_metrics::{TransportProximity, TrustMetrics};
pub use verifier_chain::VerifierChain;
pub use verifier_event::{ProximityVerifierEvent, VerifierEventLog, VerifierMethod};
#[cfg(any(test, feature = "testing"))]
pub use verifier_harness::{PeerCapabilities, Scenario, SimulatedPeer, VerificationOutcome};
pub use x3dh::{X3DH, X3DHKeyPair};

// Multi-stage exchange re-exports
pub use multistage::session::MultiStageSession;
pub use multistage::types::{ProtocolState, QrPayload};

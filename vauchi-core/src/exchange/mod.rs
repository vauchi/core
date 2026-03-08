// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

#[cfg(feature = "testing")]
pub mod audio;
#[cfg(not(feature = "testing"))]
mod audio;

#[cfg(feature = "audio-cpal")]
pub mod audio_cpal;

#[cfg(feature = "testing")]
pub mod ble;
#[cfg(not(feature = "testing"))]
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

pub mod device_link;

pub mod transport;

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

pub use audio::{AudioBackend, AudioCapability, AudioConfig, MockAudioBackend, UltrasonicVerifier};
#[cfg(feature = "audio-cpal")]
pub use audio_cpal::CpalAudioBackend;
pub use ble::{
    BLEAdvertisement, BLEDevice, BLEError, BLEExchangeSession, BLEExchangeState,
    BLEProximityVerifier, BLETransport, ExchangeBle, MockBLETransport, MockBLEVerifier,
    BLE_DEFAULT_USABLE, BLE_MIN_MTU, BLE_PAYLOAD_SIZE, CHAR_CARD_EXCHANGE, CHAR_CHALLENGE,
    CHAR_DATA_NOTIFY, CHAR_DATA_WRITE, CHAR_EXCHANGE_PAYLOAD, CHAR_HANDSHAKE_NOTIFY,
    CHAR_HANDSHAKE_WRITE, VAUCHI_BLE_DIAGNOSTIC_SERVICE_UUID, VAUCHI_BLE_SERVICE_UUID,
};
pub use ble_chunking::{BleChunker, BleReassembler, BLE_CHUNK_OVERHEAD};
pub use ble_handshake::{
    BleExchangeResult, BleHandshakeSession, BleHandshakeState, BLE_HANDSHAKE_INFO,
    BLE_HANDSHAKE_VERSION,
};
pub use ble_payload::BleCardPayload;
pub use ble_rollback::BleRollback;
pub use device_link::{
    compute_confirmation_mac, generate_numeric_code, DeviceLinkConfirmation, DeviceLinkInitiator,
    DeviceLinkInitiatorRestored, DeviceLinkQR, DeviceLinkRequest, DeviceLinkResponder,
    DeviceLinkResponse, ProximityProof,
};
pub use encrypted_message::{DecryptedExchangePayload, EncryptedExchangeMessage};
pub use error::ExchangeError;
pub use nfc_active::apdu as nfc_apdu;
pub use nfc_active::{ExchangeNfc, NFC_PAYLOAD_SIZE};
pub use nfc_apdu_chaining::{
    extract_data, is_chained, reassemble_chain, split_into_chain, MAX_APDU_DATA,
};
pub use nfc_card_payload::NfcCardPayload;
pub use nfc_handshake::{NfcExchangeResult, NfcHandshakeSession, NfcHandshakeState};
pub use nfc_rollback::{NfcRollback, NoopNfcRollback};
pub use proximity::{
    ManualConfirmationVerifier, MockProximityVerifier, ProximityConfidence, ProximityError,
    ProximityVerifier,
};
pub use qr::{check_clock_drift, ExchangeQR};
pub use session::{
    DefaultPlatformCallbacks, DuplicateAction, ExchangeEvent, ExchangePlatformCallbacks,
    ExchangeSession, ExchangeState, ExchangeTransport,
};
pub use x3dh::{X3DHKeyPair, X3DH};

// Multi-stage exchange re-exports
pub use multistage::session::MultiStageSession;
pub use multistage::types::{ProtocolState, QrPayload};

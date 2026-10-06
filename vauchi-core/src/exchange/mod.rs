// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

// These modules are `pub` under `feature = "testing"` (for integration and
// downstream test crates) and private otherwise. Each is declared on its
// own rather than through a macro so that cargo-mutants, which follows
// `mod` items without expanding macros, can find and mutate them
// (vauchi/private#522).
#[cfg(feature = "testing")]
pub mod accelerometer;
#[cfg(not(feature = "testing"))]
mod accelerometer;
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
pub mod encrypted_message;
#[cfg(not(feature = "testing"))]
mod encrypted_message;
#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;
#[cfg(feature = "testing")]
pub mod exchange_payload;
#[cfg(not(feature = "testing"))]
mod exchange_payload;
#[cfg(feature = "testing")]
pub mod multistage;
#[cfg(not(feature = "testing"))]
mod multistage;
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
pub mod trust_metrics;
#[cfg(not(feature = "testing"))]
mod trust_metrics;
#[cfg(feature = "testing")]
pub mod x3dh;
#[cfg(not(feature = "testing"))]
mod x3dh;

// ble has additional #[allow(deprecated)] (ADR-031 transport deprecation)
#[cfg(feature = "testing")]
#[allow(deprecated)]
pub mod ble;
#[cfg(not(feature = "testing"))]
#[allow(deprecated)]
mod ble;

#[cfg(feature = "audio-cpal")]
pub mod audio_cpal;
pub mod audio_modem;

pub mod capability;
pub mod confirmation_escrow;
pub mod defaults;
pub mod device_link;
pub mod direct_transport;
pub mod escrow;
pub mod exchange_id;
pub mod genesis;
pub mod key_order;
pub mod link_initiator;
pub mod link_mode;
pub mod link_responder;
pub mod mode;
pub mod mode_availability;
pub mod mode_payload;
pub mod oob_bootstrap;
pub mod proximity_runner;
pub mod ratchet_bootstrap;
pub mod reciprocity;
pub mod reciprocity_tokens;
pub mod relay_exchange;
pub mod shake_protocol;
pub mod tcp_transport;

pub mod transport;

pub use accelerometer::{
    AccelerometerBackend, AccelerometerConfig, AccelerometerSample, AccelerometerVerifier,
    MockAccelerometerBackend,
};
#[cfg(feature = "audio-cpal")]
pub use audio_cpal::CpalAudioBackend;
pub use audio_modem::AudioConfig;
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
    BleHandshakeState, KEY_ACK_SIZE, KEY_OFFER_SIZE,
};
pub use ble_payload::BleCardPayload;
pub use device_link::{
    DeviceLinkConfirmation, DeviceLinkInitiator, DeviceLinkInitiatorRestored,
    DeviceLinkJoinInvitation, DeviceLinkQR, DeviceLinkRequest, DeviceLinkResponder,
    DeviceLinkResponse, JoinInvitationError, ProximityProof, compute_confirmation_mac,
    generate_numeric_code,
};
pub use direct_transport::{ProximityLevel, UsbRole};
pub use encrypted_message::{DecryptedExchangePayload, EncryptedExchangeMessage};
pub use error::ExchangeError;
pub mod nfc_apdu;
pub use nfc_active::{ExchangeNfc, NFC_PAYLOAD_SIZE};
pub use nfc_apdu_chaining::{
    MAX_APDU_DATA, extract_data, is_chained, reassemble_chain, split_into_chain,
};
pub use nfc_card_payload::NfcCardPayload;
pub use nfc_handshake::{NfcExchangeResult, NfcHandshakeSession, NfcHandshakeState};
#[cfg(any(test, feature = "testing"))]
pub use proximity::MockProximityVerifier;
pub use proximity::{
    ManualConfirmationVerifier, ProximityConfidence, ProximityError, ProximityVerifier,
};
pub use qr::{ExchangeQR, check_clock_drift};
pub use reciprocity::{ConfirmationChannel, Reciprocity};
pub use session::{
    DefaultPlatformCallbacks, DuplicateAction, ExchangeEvent, ExchangePlatformCallbacks,
    ExchangeSession, ExchangeState, ExchangeTransport,
};
pub use trust_metrics::{TransportProximity, TrustMetrics};
pub use x3dh::{X3DH, X3DHKeyPair};

pub use multistage::session::{AccelStateError, AudioStateError, MultiStageSession};
pub use multistage::types::{
    AccelerometerProximityState, AudioProximityState, LinkFeedback, ProtocolState, QrPayload,
};

pub use defaults::ExchangeDefaults;
pub use exchange_id::ExchangeId;
pub use mode::{
    BootstrapMethod, DataTransport, DeviceRequirement, ExchangeContext, ExchangeMode, ModeCategory,
    ModeConfig, ProximityMethod,
};
pub use mode_availability::{ModeAvailability, check_mode_availability, recommend_mode};
pub use mode_payload::ExchangeModePayload;

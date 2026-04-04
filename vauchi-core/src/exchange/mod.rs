// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

/// Modules that are `pub` under `feature = "testing"` and private otherwise.
/// This allows integration tests and downstream test crates to access internals
/// while keeping them private in production builds.
macro_rules! test_pub_mod {
    ($($name:ident),+ $(,)?) => {
        $(
            #[cfg(feature = "testing")]
            pub mod $name;
            #[cfg(not(feature = "testing"))]
            mod $name;
        )+
    };
}

test_pub_mod!(
    accelerometer,
    ambient_audio,
    audio,
    ble_chunking,
    ble_handshake,
    ble_payload,
    ble_rollback,
    encrypted_message,
    error,
    exchange_payload,
    multistage,
    nfc_active,
    nfc_apdu_chaining,
    nfc_card_payload,
    nfc_handshake,
    nfc_rollback,
    proximity,
    qr,
    session,
    trust_metrics,
    verifier_chain,
    verifier_event,
    x3dh,
);

// ble has additional #[allow(deprecated)] (ADR-031 transport deprecation)
#[cfg(feature = "testing")]
#[allow(deprecated)]
pub mod ble;
#[cfg(not(feature = "testing"))]
#[allow(deprecated)]
mod ble;

#[cfg(feature = "audio-cpal")]
pub mod audio_cpal;

pub mod capability;
pub mod card_snapshot;
pub mod command;
pub mod device_link;
pub mod escrow;
pub mod exchange_id;
pub mod exchange_record;
pub mod link_mode;
pub mod mode;
pub mod mode_availability;
pub mod mode_payload;
pub mod persisted_state;
pub mod reciprocity;
pub mod relay_exchange;
pub mod tcp_transport;

#[allow(deprecated)]
pub mod transport;

#[cfg(any(test, feature = "testing"))]
pub mod verifier_harness;

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
pub use reciprocity::{ConfirmationChannel, Reciprocity};
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

// Exchange mode foundation re-exports
pub use card_snapshot::CardSnapshot;
pub use exchange_id::ExchangeId;
pub use exchange_record::{
    ExchangeRecord, ExchangeTrustLevel, ProximityResult, ReverificationRecord,
};
pub use mode::{
    BootstrapMethod, DataTransport, DeviceRequirement, ExchangeContext, ExchangeMode, ModeCategory,
    ModeConfig, ProximityMethod,
};
pub use mode_availability::{ModeAvailability, check_mode_availability, recommend_mode};
pub use mode_payload::ExchangeModePayload;
pub use persisted_state::{ExchangeLifecycleState, PersistedExchangeState};

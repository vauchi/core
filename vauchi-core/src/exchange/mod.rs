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

pub mod device_link;

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
pub mod nfc;
#[cfg(not(feature = "testing"))]
mod nfc;

#[cfg(feature = "testing")]
pub mod x3dh;
#[cfg(not(feature = "testing"))]
mod x3dh;

pub use audio::{AudioBackend, AudioCapability, AudioConfig, MockAudioBackend, UltrasonicVerifier};
#[cfg(feature = "audio-cpal")]
pub use audio_cpal::CpalAudioBackend;
pub use ble::{
    BLEAdvertisement, BLEDevice, BLEError, BLEExchangeSession, BLEExchangeState,
    BLEProximityVerifier, MockBLEVerifier, VAUCHI_BLE_SERVICE_UUID,
};
pub use device_link::{
    generate_numeric_code, DeviceLinkInitiator, DeviceLinkInitiatorRestored, DeviceLinkQR,
    DeviceLinkRequest, DeviceLinkResponder, DeviceLinkResponse,
};
pub use encrypted_message::{DecryptedExchangePayload, EncryptedExchangeMessage};
pub use error::ExchangeError;
pub use nfc::{
    create_nfc_tag, parse_nfc_payload, Introduction, NfcError, NfcTagCreationResult, NfcTagMode,
    NfcTagPayload, ParsedNfcPayload, ProtectedNfcTagPayload,
};
pub use proximity::{
    ManualConfirmationVerifier, MockProximityVerifier, ProximityError, ProximityVerifier,
};
pub use qr::{check_clock_drift, ExchangeQR};
pub use session::{
    DefaultPlatformCallbacks, DuplicateAction, ExchangeEvent, ExchangeMode,
    ExchangePlatformCallbacks, ExchangeRole, ExchangeSession, ExchangeState,
};
pub use x3dh::{X3DHKeyPair, X3DH};

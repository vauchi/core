//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

mod audio;
mod ble;
pub mod device_link;
mod error;
mod proximity;
mod qr;
mod session;
mod x3dh;

pub use audio::{AudioBackend, AudioCapability, AudioConfig, MockAudioBackend, UltrasonicVerifier};
pub use ble::{BLEDevice, BLEProximityVerifier, MockBLEVerifier};
pub use device_link::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkRequest, DeviceLinkResponder, DeviceLinkResponse,
};
pub use error::ExchangeError;
pub use proximity::{
    ManualConfirmationVerifier, MockProximityVerifier, ProximityError, ProximityVerifier,
};
pub use qr::ExchangeQR;
pub use session::{DuplicateAction, ExchangeRole, ExchangeSession, ExchangeState};
pub use x3dh::{X3DHKeyPair, X3DH};

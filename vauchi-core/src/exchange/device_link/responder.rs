// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link responder state machine (new device side).

use super::super::ExchangeError;
use super::qr::DeviceLinkQR;
use super::request::DeviceLinkRequest;
use super::response::DeviceLinkResponse;
use super::types::{derive_confirmation_code, derive_proximity_challenge};

/// State machine for device linking from the new device's perspective.
pub struct DeviceLinkResponder {
    /// The scanned QR code
    qr: DeviceLinkQR,
    /// The device name for this new device
    device_name: String,
    /// Nonce from the last created request (for confirmation code computation).
    last_request_nonce: Option<[u8; 32]>,
}

impl DeviceLinkResponder {
    /// Creates a new responder after scanning a device link QR.
    pub fn from_qr(qr: DeviceLinkQR, device_name: String, now: u64) -> Result<Self, ExchangeError> {
        if qr.is_expired(now) {
            return Err(ExchangeError::TokenExpired);
        }

        Ok(DeviceLinkResponder {
            qr,
            device_name,
            last_request_nonce: None,
        })
    }

    /// Derives a 16-byte proximity challenge from the link key.
    ///
    /// Both the initiator and responder derive the same challenge from the
    /// shared link key, enabling proximity verification.
    pub fn proximity_challenge(&self) -> [u8; 16] {
        derive_proximity_challenge(self.qr.link_key())
    }

    /// Creates a request to send to the existing device.
    pub fn create_request(&mut self, now: u64) -> Result<Vec<u8>, ExchangeError> {
        let request = DeviceLinkRequest::new(self.device_name.clone(), now);
        self.last_request_nonce = Some(request.nonce);
        request.encrypt(self.qr.link_key())
    }

    /// Computes the confirmation code that should match the initiator's display.
    ///
    /// Must be called after `create_request()`. Both devices derive the same code
    /// from the shared link key and request nonce.
    pub fn compute_confirmation_code(&self) -> Result<String, ExchangeError> {
        let nonce = self
            .last_request_nonce
            .ok_or(ExchangeError::InvalidQRFormat)?;
        Ok(derive_confirmation_code(self.qr.link_key(), &nonce))
    }

    /// Returns the identity fingerprint from the QR code.
    ///
    /// Should match the fingerprint shown on the initiator's confirmation screen.
    pub fn identity_fingerprint(&self) -> String {
        self.qr.identity_fingerprint()
    }

    /// Processes the response from the existing device.
    ///
    /// Returns the master seed, display name, device index, and registry.
    pub fn process_response(
        &self,
        encrypted_response: &[u8],
    ) -> Result<DeviceLinkResponse, ExchangeError> {
        DeviceLinkResponse::decrypt(encrypted_response, self.qr.link_key())
    }

    /// Returns the identity public key from the QR.
    pub fn identity_public_key(&self) -> &[u8; 32] {
        self.qr.identity_public_key()
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Device link responder state machine (new device side).

use super::super::ExchangeError;
use super::qr::DeviceLinkQR;
use super::refusal::{DeviceLinkRefusalReason, decrypt_refusal};
use super::request::DeviceLinkRequest;
use super::response::DeviceLinkResponse;
use super::types::derive_confirmation_code;
use crate::crypto::SigningKeyPair;
use crate::identity::DeviceInfo;
use zeroize::Zeroizing;

/// State machine for device linking from the new device's perspective.
pub struct DeviceLinkResponder {
    /// The scanned QR code
    qr: DeviceLinkQR,
    /// The device name for this new device
    device_name: String,
    /// Nonce from the last created request (for confirmation code computation).
    last_request_nonce: Option<[u8; 32]>,
    /// This device's own signing key, kept here until the seed arrives and
    /// storage can hold it (ADR-079).
    device_signing_seed: Zeroizing<[u8; 32]>,
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
            device_signing_seed: Zeroizing::new(crate::crypto::random_bytes()),
        })
    }

    /// Creates a request to send to the existing device.
    pub fn create_request(&mut self, now: u64) -> Result<Vec<u8>, ExchangeError> {
        let device_key = SigningKeyPair::from_seed(&self.device_signing_seed);
        let request = DeviceLinkRequest::new(
            self.device_name.clone(),
            *device_key.public_key().as_bytes(),
            now,
        );
        self.last_request_nonce = Some(request.nonce);
        request.encrypt(self.qr.link_key())
    }

    /// Computes the confirmation code that should match the initiator's display.
    ///
    /// Must be called after `create_request()`. Both devices derive the same code
    /// from the shared link key, request nonce and this device's public key.
    pub fn compute_confirmation_code(&self) -> Result<String, ExchangeError> {
        let nonce = self
            .last_request_nonce
            .ok_or(ExchangeError::InvalidQRFormat)?;
        let device_key = SigningKeyPair::from_seed(&self.device_signing_seed);
        Ok(derive_confirmation_code(
            self.qr.link_key(),
            &nonce,
            device_key.public_key().as_bytes(),
        ))
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
    /// Refuses a seed whose identity key is not the QR's: anyone with a
    /// copy of the QR holds the link key (vauchi/private#789). Refuses a
    /// registry that does not name this device's own key, because contacts
    /// would pin whatever it names.
    pub fn process_response(
        &self,
        encrypted_response: &[u8],
    ) -> Result<DeviceLinkResponse, ExchangeError> {
        let mut response = DeviceLinkResponse::decrypt(encrypted_response, self.qr.link_key())?;
        let seed_identity_key = SigningKeyPair::from_seed(response.master_seed()).public_key();
        if seed_identity_key.as_bytes() != self.qr.identity_public_key() {
            return Err(ExchangeError::IdentityMismatch);
        }
        let device_key = SigningKeyPair::from_seed(&self.device_signing_seed);
        let this_device = DeviceInfo::derive(
            response.master_seed(),
            response.device_index(),
            self.device_name.clone(),
            0,
        );
        let named_key = response
            .registry()
            .find_device(this_device.device_id())
            .filter(|entry| entry.is_active())
            .and_then(|entry| entry.signing_public_key);
        if named_key != Some(*device_key.public_key().as_bytes())
            || !response.registry().verify(&seed_identity_key)
        {
            return Err(ExchangeError::DeviceKeyRefused);
        }
        response.set_device_signing_seed(self.device_signing_seed.clone());
        Ok(response)
    }

    /// Reads the host's word that it ended the ceremony, instead of a
    /// response (vauchi/private#780).
    pub fn process_refusal(
        &self,
        encrypted_refusal: &[u8],
    ) -> Result<DeviceLinkRefusalReason, ExchangeError> {
        decrypt_refusal(self.qr.link_key(), encrypted_refusal)
    }

    /// Returns the identity public key from the QR.
    pub fn identity_public_key(&self) -> &[u8; 32] {
        self.qr.identity_public_key()
    }
}

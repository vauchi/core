// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link initiator state machines (existing device side).

use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::identity::{DeviceInfo, DeviceRegistry, Identity};

use super::super::ExchangeError;
use super::qr::DeviceLinkQR;
use super::request::DeviceLinkRequest;
use super::response::DeviceLinkResponse;
use super::types::{
    DeviceLinkConfirmation, PROXIMITY_PROOF_MAX_AGE_SECS, ProximityProof, compute_confirmation_mac,
    derive_confirmation_code, derive_proximity_challenge,
};

/// State machine for device linking from the existing device's perspective.
pub struct DeviceLinkInitiator {
    /// The identity on this device (reserved for future verification)
    _identity_public_key: [u8; 32],
    /// Master seed to transfer (kept for creating response)
    master_seed: [u8; 32],
    /// Display name to transfer
    display_name: String,
    /// The generated QR code
    qr: DeviceLinkQR,
    /// Current device registry
    registry: DeviceRegistry,
}

impl DeviceLinkInitiator {
    /// Creates a new device link initiator.
    ///
    /// The identity parameter is used to get the master seed for transfer.
    /// In a real implementation, we'd need a way to access the master seed
    /// from the identity - this is intentionally designed to require explicit
    /// seed access for security.
    pub fn new(master_seed: [u8; 32], identity: &Identity, registry: DeviceRegistry) -> Self {
        let qr = DeviceLinkQR::generate(identity);

        DeviceLinkInitiator {
            _identity_public_key: *identity.signing_public_key(),
            master_seed,
            display_name: identity.display_name().to_string(),
            qr,
            registry,
        }
    }

    /// Returns the QR code to display.
    pub fn qr(&self) -> &DeviceLinkQR {
        &self.qr
    }

    /// Derives a 16-byte proximity challenge from the link key.
    ///
    /// Both the initiator and responder can derive the same challenge since
    /// they share the link key (from QR). The challenge is used for proximity
    /// verification (e.g., ultrasonic audio or manual confirmation).
    pub fn proximity_challenge(&self) -> [u8; 16] {
        derive_proximity_challenge(self.qr.link_key())
    }

    /// Decrypts a link request and returns confirmation details for the user.
    ///
    /// The caller must display `DeviceLinkConfirmation` to the user (device name,
    /// confirmation code, identity fingerprint) and get explicit approval before
    /// calling `confirm_link()`.
    ///
    /// Both devices independently display the same confirmation code, allowing the
    /// user to verify the link is legitimate even without NFC/camera/bluetooth/audio.
    pub fn prepare_confirmation(
        &self,
        encrypted_request: &[u8],
    ) -> Result<(DeviceLinkConfirmation, DeviceLinkRequest), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        let identity_fingerprint = self.qr.identity_fingerprint();

        let confirmation = DeviceLinkConfirmation {
            device_name: request.device_name.clone(),
            confirmation_code,
            identity_fingerprint,
        };

        Ok((confirmation, request))
    }

    /// After the user confirms the link, creates the encrypted response with the
    /// master seed and returns the updated registry and new device info.
    ///
    /// Call `prepare_confirmation()` first to get the `DeviceLinkRequest`.
    pub fn confirm_link(
        &self,
        request: &DeviceLinkRequest,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, None, proof)
    }

    /// After the user confirms the link, creates the encrypted response with the
    /// master seed and sync payload.
    ///
    /// Call `prepare_confirmation()` first to get the `DeviceLinkRequest`.
    pub fn confirm_link_with_sync(
        &self,
        request: &DeviceLinkRequest,
        sync_payload_json: &str,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, Some(sync_payload_json), proof)
    }

    /// Processes a link request and creates a response.
    ///
    /// Returns the encrypted response and the updated registry with the new device.
    #[deprecated(note = "Use prepare_confirmation() + confirm_link() for user verification")]
    pub fn process_request(
        &self,
        encrypted_request: &[u8],
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        self.build_response(&request, None, proof)
    }

    /// Processes a link request and creates a response with sync payload.
    ///
    /// This variant includes the full sync payload for the new device.
    #[deprecated(
        note = "Use prepare_confirmation() + confirm_link_with_sync() for user verification"
    )]
    pub fn process_request_with_sync(
        &self,
        encrypted_request: &[u8],
        sync_payload_json: &str,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        self.build_response(&request, Some(sync_payload_json), proof)
    }

    /// Internal helper to build the response from a validated request.
    ///
    /// Validates the proximity proof cryptographically before releasing the
    /// master seed. Returns `Err(ExchangeError::ProximityExpired)` if the
    /// proof is too old, or `Err(ExchangeError::ProximityNotVerified)` if
    /// the proof data does not match.
    fn build_response(
        &self,
        request: &DeviceLinkRequest,
        sync_payload_json: Option<&str>,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // DL-6: Reject self-linking — device name must not match any active device.
        // Note: This is a name-based heuristic. Proper enforcement requires a
        // protocol change to include device_id in DeviceLinkRequest.
        if self
            .registry
            .active_devices()
            .iter()
            .any(|d| d.device_name == request.device_name)
        {
            return Err(ExchangeError::SelfLinkingNotAllowed);
        }

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        self.validate_proximity_proof(proof, &confirmation_code)?;

        let device_index = self.registry.next_device_index();

        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        let response = match sync_payload_json {
            Some(payload) => DeviceLinkResponse::with_sync_payload(
                self.master_seed,
                self.display_name.clone(),
                device_index,
                updated_registry.clone(),
                payload.to_string(),
            ),
            None => DeviceLinkResponse::new(
                self.master_seed,
                self.display_name.clone(),
                device_index,
                updated_registry.clone(),
            ),
        };

        let encrypted_response = response.encrypt(self.qr.link_key())?;
        let new_device =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        Ok((encrypted_response, updated_registry, new_device))
    }

    /// Validates a proximity proof cryptographically.
    ///
    /// Checks both freshness (proof age within `PROXIMITY_PROOF_MAX_AGE_SECS`)
    /// and correctness (challenge-response or confirmation MAC) using
    /// constant-time comparison.
    fn validate_proximity_proof(
        &self,
        proof: &ProximityProof,
        confirmation_code: &str,
    ) -> Result<(), ExchangeError> {
        let now = crate::exchange::now_secs();

        match proof {
            ProximityProof::Ultrasonic {
                challenge_response,
                verified_at,
            } => {
                if now.saturating_sub(*verified_at) > PROXIMITY_PROOF_MAX_AGE_SECS {
                    return Err(ExchangeError::ProximityExpired);
                }
                let expected = derive_proximity_challenge(self.qr.link_key());
                if !bool::from(challenge_response.ct_eq(&expected)) {
                    return Err(ExchangeError::ProximityNotVerified);
                }
                Ok(())
            }
            ProximityProof::ManualConfirmation {
                confirmation_code_mac,
                confirmed_at,
            } => {
                if now.saturating_sub(*confirmed_at) > PROXIMITY_PROOF_MAX_AGE_SECS {
                    return Err(ExchangeError::ProximityExpired);
                }
                let expected_mac = compute_confirmation_mac(self.qr.link_key(), confirmation_code);
                if !bool::from(confirmation_code_mac.ct_eq(&expected_mac)) {
                    return Err(ExchangeError::ProximityNotVerified);
                }
                Ok(())
            }
        }
    }
}

impl Drop for DeviceLinkInitiator {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

/// State machine for device linking from the existing device's perspective (restored from saved QR).
///
/// Used when the QR was generated earlier and saved to disk, then restored
/// when the request comes in.
pub struct DeviceLinkInitiatorRestored {
    /// Master seed to transfer
    master_seed: [u8; 32],
    /// Display name to transfer
    display_name: String,
    /// The restored QR code
    qr: DeviceLinkQR,
    /// Current device registry
    registry: DeviceRegistry,
}

impl DeviceLinkInitiatorRestored {
    /// Creates a restored initiator with a previously saved QR code.
    pub fn new(
        master_seed: [u8; 32],
        identity: &Identity,
        registry: DeviceRegistry,
        qr: DeviceLinkQR,
    ) -> Self {
        DeviceLinkInitiatorRestored {
            master_seed,
            display_name: identity.display_name().to_string(),
            qr,
            registry,
        }
    }

    /// Returns the QR code.
    pub fn qr(&self) -> &DeviceLinkQR {
        &self.qr
    }

    /// Derives a 16-byte proximity challenge from the link key.
    ///
    /// See `DeviceLinkInitiator::proximity_challenge()` for details.
    pub fn proximity_challenge(&self) -> [u8; 16] {
        derive_proximity_challenge(self.qr.link_key())
    }

    /// Decrypts a link request and returns confirmation details for the user.
    ///
    /// See `DeviceLinkInitiator::prepare_confirmation()` for details.
    pub fn prepare_confirmation(
        &self,
        encrypted_request: &[u8],
    ) -> Result<(DeviceLinkConfirmation, DeviceLinkRequest), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        let identity_fingerprint = self.qr.identity_fingerprint();

        let confirmation = DeviceLinkConfirmation {
            device_name: request.device_name.clone(),
            confirmation_code,
            identity_fingerprint,
        };

        Ok((confirmation, request))
    }

    /// After the user confirms the link, creates the encrypted response.
    pub fn confirm_link(
        &self,
        request: &DeviceLinkRequest,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, None, proof)
    }

    /// After the user confirms the link, creates the encrypted response with sync payload.
    pub fn confirm_link_with_sync(
        &self,
        request: &DeviceLinkRequest,
        sync_payload_json: &str,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, Some(sync_payload_json), proof)
    }

    /// Processes a link request and creates a response.
    #[deprecated(note = "Use prepare_confirmation() + confirm_link() for user verification")]
    pub fn process_request(
        &self,
        encrypted_request: &[u8],
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        self.build_response(&request, None, proof)
    }

    /// Processes a link request with sync payload and creates a response.
    #[deprecated(
        note = "Use prepare_confirmation() + confirm_link_with_sync() for user verification"
    )]
    pub fn process_request_with_sync(
        &self,
        encrypted_request: &[u8],
        sync_payload_json: &str,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        self.build_response(&request, Some(sync_payload_json), proof)
    }

    /// Internal helper to build the response from a validated request.
    ///
    /// Validates the proximity proof cryptographically before releasing the
    /// master seed. Returns `Err(ExchangeError::ProximityExpired)` if the
    /// proof is too old, or `Err(ExchangeError::ProximityNotVerified)` if
    /// the proof data does not match.
    fn build_response(
        &self,
        request: &DeviceLinkRequest,
        sync_payload_json: Option<&str>,
        proof: &ProximityProof,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // DL-6: Reject self-linking — device name must not match any active device.
        // Note: This is a name-based heuristic. Proper enforcement requires a
        // protocol change to include device_id in DeviceLinkRequest.
        if self
            .registry
            .active_devices()
            .iter()
            .any(|d| d.device_name == request.device_name)
        {
            return Err(ExchangeError::SelfLinkingNotAllowed);
        }

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        self.validate_proximity_proof(proof, &confirmation_code)?;

        let device_index = self.registry.next_device_index();

        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        let response = match sync_payload_json {
            Some(payload) => DeviceLinkResponse::with_sync_payload(
                self.master_seed,
                self.display_name.clone(),
                device_index,
                updated_registry.clone(),
                payload.to_string(),
            ),
            None => DeviceLinkResponse::new(
                self.master_seed,
                self.display_name.clone(),
                device_index,
                updated_registry.clone(),
            ),
        };

        let encrypted_response = response.encrypt(self.qr.link_key())?;
        let new_device =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        Ok((encrypted_response, updated_registry, new_device))
    }

    /// Validates a proximity proof cryptographically.
    ///
    /// Checks both freshness (proof age within `PROXIMITY_PROOF_MAX_AGE_SECS`)
    /// and correctness (challenge-response or confirmation MAC) using
    /// constant-time comparison.
    fn validate_proximity_proof(
        &self,
        proof: &ProximityProof,
        confirmation_code: &str,
    ) -> Result<(), ExchangeError> {
        let now = crate::exchange::now_secs();

        match proof {
            ProximityProof::Ultrasonic {
                challenge_response,
                verified_at,
            } => {
                if now.saturating_sub(*verified_at) > PROXIMITY_PROOF_MAX_AGE_SECS {
                    return Err(ExchangeError::ProximityExpired);
                }
                let expected = derive_proximity_challenge(self.qr.link_key());
                if !bool::from(challenge_response.ct_eq(&expected)) {
                    return Err(ExchangeError::ProximityNotVerified);
                }
                Ok(())
            }
            ProximityProof::ManualConfirmation {
                confirmation_code_mac,
                confirmed_at,
            } => {
                if now.saturating_sub(*confirmed_at) > PROXIMITY_PROOF_MAX_AGE_SECS {
                    return Err(ExchangeError::ProximityExpired);
                }
                let expected_mac = compute_confirmation_mac(self.qr.link_key(), confirmation_code);
                if !bool::from(confirmation_code_mac.ct_eq(&expected_mac)) {
                    return Err(ExchangeError::ProximityNotVerified);
                }
                Ok(())
            }
        }
    }
}

impl Drop for DeviceLinkInitiatorRestored {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

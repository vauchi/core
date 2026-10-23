// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device link initiator state machines (existing device side).

use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::crypto::SigningKeyPair;
use crate::identity::{DeviceInfo, DeviceRegistry, Identity};

use super::super::ExchangeError;
use super::qr::DeviceLinkQR;
use super::refusal::{DeviceLinkRefusalReason, encrypt_refusal};
use super::request::DeviceLinkRequest;
use super::response::DeviceLinkResponse;
use super::types::{
    DeviceLinkConfirmation, PROXIMITY_PROOF_MAX_AGE_SECS, ProximityProof, compute_confirmation_mac,
    derive_confirmation_code,
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
    pub fn new(
        master_seed: [u8; 32],
        identity: &Identity,
        registry: DeviceRegistry,
        now: u64,
    ) -> Self {
        let qr = DeviceLinkQR::generate(identity, now);

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

    /// Tells the joiner, on its return channel, that this host ended the
    /// ceremony and why (vauchi/private#780).
    pub fn refuse(&self, reason: DeviceLinkRefusalReason) -> Result<Vec<u8>, ExchangeError> {
        encrypt_refusal(self.qr.link_key(), reason)
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
        // Before the code is shown, so the user is never asked to confirm a
        // link that `build_response` would refuse (vauchi/private#671).
        refuse_a_taken_device_name(&self.registry, &request.device_name)?;

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
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, None, proof, now)
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
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, Some(sync_payload_json), proof, now)
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
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        refuse_a_taken_device_name(&self.registry, &request.device_name)?;

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        validate_proximity_proof(self.qr.link_key(), proof, &confirmation_code, now)?;

        let device_index = self.registry.next_device_index();

        let new_device_info = DeviceInfo::derive(
            &self.master_seed,
            device_index,
            request.device_name.clone(),
            now,
        );

        let mut updated_registry = self.registry.clone();
        let signing_key = SigningKeyPair::from_seed(&self.master_seed);
        updated_registry
            .add_device(
                new_device_info.to_registered(&self.master_seed),
                &signing_key,
            )
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
        let new_device = DeviceInfo::derive(
            &self.master_seed,
            device_index,
            request.device_name.clone(),
            now,
        );

        Ok((encrypted_response, updated_registry, new_device))
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
        // Before the code is shown, so the user is never asked to confirm a
        // link that `build_response` would refuse (vauchi/private#671).
        refuse_a_taken_device_name(&self.registry, &request.device_name)?;

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
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, None, proof, now)
    }

    /// After the user confirms the link, creates the encrypted response with sync payload.
    pub fn confirm_link_with_sync(
        &self,
        request: &DeviceLinkRequest,
        sync_payload_json: &str,
        proof: &ProximityProof,
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        self.build_response(request, Some(sync_payload_json), proof, now)
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
        now: u64,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        refuse_a_taken_device_name(&self.registry, &request.device_name)?;

        let confirmation_code = derive_confirmation_code(self.qr.link_key(), &request.nonce);
        validate_proximity_proof(self.qr.link_key(), proof, &confirmation_code, now)?;

        let device_index = self.registry.next_device_index();

        let new_device_info = DeviceInfo::derive(
            &self.master_seed,
            device_index,
            request.device_name.clone(),
            now,
        );

        let mut updated_registry = self.registry.clone();
        let signing_key = SigningKeyPair::from_seed(&self.master_seed);
        updated_registry
            .add_device(
                new_device_info.to_registered(&self.master_seed),
                &signing_key,
            )
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
        let new_device = DeviceInfo::derive(
            &self.master_seed,
            device_index,
            request.device_name.clone(),
            now,
        );

        Ok((encrypted_response, updated_registry, new_device))
    }
}

impl Drop for DeviceLinkInitiatorRestored {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

/// Validates a proximity proof cryptographically.
///
/// Checks both freshness (proof age within `PROXIMITY_PROOF_MAX_AGE_SECS`)
/// and correctness (challenge-response or confirmation MAC) using
/// constant-time comparison. Shared by both initiators so the check
/// cannot drift between them.
fn validate_proximity_proof(
    link_key: &[u8; 32],
    proof: &ProximityProof,
    confirmation_code: &str,
    now: u64,
) -> Result<(), ExchangeError> {
    match proof {
        ProximityProof::ManualConfirmation {
            confirmation_code_mac,
            confirmed_at,
        } => {
            if now.saturating_sub(*confirmed_at) > PROXIMITY_PROOF_MAX_AGE_SECS {
                return Err(ExchangeError::ProximityExpired);
            }
            let expected_mac = compute_confirmation_mac(link_key, confirmation_code);
            if !bool::from(confirmation_code_mac.ct_eq(&expected_mac)) {
                return Err(ExchangeError::ProximityNotVerified);
            }
            Ok(())
        }
    }
}

/// DL-6: rejects self-linking. Name-based, because the request carries no
/// device id; proper enforcement needs that protocol change.
fn refuse_a_taken_device_name(
    registry: &DeviceRegistry,
    device_name: &str,
) -> Result<(), ExchangeError> {
    if registry
        .active_devices()
        .iter()
        .any(|d| d.device_name == device_name)
    {
        return Err(ExchangeError::SelfLinkingNotAllowed);
    }
    Ok(())
}

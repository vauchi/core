// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking, relay transport, and multipart QR operations for mobile.

use std::sync::Arc;
use std::sync::Mutex;

use vauchi_core::exchange::device_link::{DeviceLinkQR, DeviceLinkResponder};

use super::error::MobileError;
use super::types::{
    MobileDeviceInfo, MobileDeviceLinkData, MobileDeviceLinkInfo, MobileDeviceLinkRequest,
};
use super::{
    MobileDeviceLinkInitiator, MobileDeviceLinkResponder, MobileDeviceLinkSession, VauchiPlatform,
};
use crate::mobile_device_link_session;
use vauchi_app::orchestrator::device_link_relay;

#[uniffi::export]
impl VauchiPlatform {
    // === Device Linking Operations ===

    /// Get list of linked devices.
    ///
    /// Returns information about all devices linked to this identity.
    /// The first device (index 0) is the primary device.
    #[deprecated(note = "Use PlatformAppEngine::get_devices instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn get_devices(&self) -> Result<Vec<MobileDeviceInfo>, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;

        // Load device registry from storage
        let registry = match storage.load_device_registry()? {
            Some(r) => r,
            None => {
                // Return just the current device if no registry exists
                let device_info = identity.device_info();
                return Ok(vec![MobileDeviceInfo {
                    device_index: device_info.device_index(),
                    device_name: device_info.device_name().to_string(),
                    is_current: true,
                    is_active: true,
                    public_key_prefix: hex::encode(&device_info.device_id()[..8]),
                    created_at: device_info.created_at(),
                }]);
            }
        };

        let current_device_id = identity.device_info().device_id();
        let devices = registry
            .all_devices()
            .iter()
            .enumerate()
            .map(
                |(idx, d): (usize, &vauchi_core::identity::RegisteredDevice)| MobileDeviceInfo {
                    device_index: idx as u32,
                    device_name: d.device_name.clone(),
                    is_current: d.device_id == *current_device_id,
                    is_active: d.is_active(),
                    public_key_prefix: hex::encode(&d.device_id[..8]),
                    created_at: d.created_at,
                },
            )
            .collect();

        Ok(devices)
    }

    /// Generate a device link QR code.
    ///
    /// Display this QR code on the existing device for a new device to scan.
    /// The QR expires after 5 minutes (per ADR-035).
    #[deprecated(note = "Use PlatformAppEngine::generate_device_link_qr instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn generate_device_link_qr(&self) -> Result<MobileDeviceLinkData, MobileError> {
        let identity = self.get_identity()?;

        let qr = DeviceLinkQR::generate(
            &identity,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let qr_data = qr.to_data_string();

        Ok(MobileDeviceLinkData {
            qr_data,
            identity_public_key: hex::encode(identity.signing_public_key()),
            timestamp: qr.timestamp(),
            expires_at: qr.expires_at(),
        })
    }

    /// Parse a device link QR code.
    ///
    /// Call this on the new device after scanning the QR code displayed
    /// on an existing device. Returns information about the identity
    /// to link with.
    #[deprecated(note = "Use PlatformAppEngine::parse_device_link_qr instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn parse_device_link_qr(
        &self,
        qr_data: String,
    ) -> Result<MobileDeviceLinkInfo, MobileError> {
        let qr =
            DeviceLinkQR::from_data_string(&qr_data).map_err(|_| MobileError::InvalidInput {
                field: "qr".to_string(),
                detail: "Invalid QR code".to_string(),
            })?;

        Ok(MobileDeviceLinkInfo {
            identity_public_key: hex::encode(qr.identity_public_key()),
            timestamp: qr.timestamp(),
            is_expired: qr.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
        })
    }

    /// Create a device-link session for the existing device
    /// (initiator side).
    ///
    /// This is the orchestrator entry point — frontends register a
    /// `DeviceLinkSessionListener`, call `start()` on the returned
    /// session, and forward user actions via `confirm_manual` /
    /// `confirm_ultrasonic` / `deny`. The session owns the relay-poll
    /// loop, the QR-expiry deadline, and the user-confirm gate.
    /// Replaces the legacy split between `start_device_link()`,
    /// `listen_for_device_link_request()`, and
    /// `send_device_link_response()`.
    ///
    /// Persistence: the session saves the updated `DeviceRegistry`
    /// after `confirm_link` succeeds, closing a pre-existing gap
    /// where the legacy single-shot path discarded it.
    #[deprecated(
        note = "Use PlatformAppEngine::create_device_link_session_initiator instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0."
    )]
    pub fn create_device_link_session_initiator(
        &self,
    ) -> Result<Arc<MobileDeviceLinkSession>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let registry = storage
            .load_device_registry()?
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(
            registry,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let identity_id = hex::encode(identity.signing_public_key());

        let transport = self
            .open_vauchi_for_relay()?
            .build_relay_transport(self.relay_url.clone(), 10_000);

        // ADR-035: device-link QR expiry is 300 s. Use the same
        // value as the relay-listen budget so the cycle thread's
        // deadline aligns with the QR expiry observed by the peer.
        const RELAY_TIMEOUT_SECS: u64 = 300;

        Ok(Arc::new(
            mobile_device_link_session::MobileDeviceLinkSession::with_persistence_initiator(
                initiator,
                transport,
                identity_id,
                RELAY_TIMEOUT_SECS,
                self.storage_path.clone(),
                self.storage_key.clone(),
            ),
        ))
    }

    /// Start a device link as the existing device (initiator).
    ///
    /// Returns a `MobileDeviceLinkInitiator` that holds the QR data and can
    /// process incoming link requests from new devices.
    #[deprecated(note = "Use create_device_link_session_initiator (orchestrator). \
                Will be removed in Phase 3 of the device-link orchestrator rollout.")]
    pub fn start_device_link(&self) -> Result<Arc<MobileDeviceLinkInitiator>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let registry = storage
            .load_device_registry()?
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(
            registry,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );

        Ok(Arc::new(MobileDeviceLinkInitiator {
            inner: Mutex::new(initiator),
            pending_request: Mutex::new(None),
        }))
    }

    /// Start a device join as the new device (responder).
    ///
    /// Parses the QR data scanned from the existing device and returns a
    /// `MobileDeviceLinkResponder` that can create requests and process responses.
    #[deprecated(note = "Reserved for the deferred responder-side orchestrator. \
                Will be replaced by create_device_link_session_responder \
                in a future Phase.")]
    pub fn start_device_join(
        &self,
        qr_data: String,
        device_name: String,
    ) -> Result<Arc<MobileDeviceLinkResponder>, MobileError> {
        let qr =
            DeviceLinkQR::from_data_string(&qr_data).map_err(|_| MobileError::InvalidInput {
                field: "qr".to_string(),
                detail: "Invalid QR code".to_string(),
            })?;

        let responder = DeviceLinkResponder::from_qr(
            qr,
            device_name,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })?;

        Ok(Arc::new(MobileDeviceLinkResponder {
            inner: Mutex::new(responder),
        }))
    }

    // === Device Link Relay Transport ===

    /// Send device link request via relay and wait for response (new device / responder).
    ///
    /// Uses two HTTP exchange cycles: creates a return channel, claims the
    /// existing device's offer with our request, then polls for the response.
    #[deprecated(note = "Reserved for the deferred responder-side orchestrator. \
                Will be replaced by create_device_link_session_responder \
                in a future Phase.")]
    pub fn send_device_link_request(
        &self,
        _target_identity: String,
        sender_token: String,
        encrypted_request: Vec<u8>,
        timeout_secs: u64,
    ) -> Result<Vec<u8>, MobileError> {
        let msg = device_link_relay::DeviceLinkRelayMessage {
            target_identity: _target_identity,
            sender_token,
            payload: encrypted_request,
        };

        let transport = self
            .open_vauchi_for_relay()?
            .build_relay_transport(self.relay_url.clone(), 10_000);
        device_link_relay::send_and_receive(&transport, &msg, timeout_secs).map_err(|e| {
            MobileError::Other {
                detail: e.to_string(),
            }
        })
    }

    /// Listen for incoming device link request via relay (existing device / initiator).
    ///
    /// Creates an exchange offer with our identity, then polls until the new
    /// device claims it. Returns the encrypted request and a token for the response.
    #[deprecated(note = "Use create_device_link_session_initiator (orchestrator). \
                Will be removed in Phase 3 of the device-link orchestrator rollout.")]
    pub fn listen_for_device_link_request(
        &self,
        timeout_secs: u64,
    ) -> Result<MobileDeviceLinkRequest, MobileError> {
        let identity = self.get_identity()?;
        let identity_id = hex::encode(identity.signing_public_key());

        let transport = self
            .open_vauchi_for_relay()?
            .build_relay_transport(self.relay_url.clone(), 10_000);
        let (payload, sender_token) =
            device_link_relay::listen_for_request(&transport, &identity_id, timeout_secs).map_err(
                |e| MobileError::Other {
                    detail: e.to_string(),
                },
            )?;

        Ok(MobileDeviceLinkRequest {
            encrypted_payload: payload,
            sender_token,
        })
    }

    /// Send device link response back via relay (existing device / initiator).
    ///
    /// Claims the return channel created by the new device, depositing the
    /// encrypted response payload.
    #[deprecated(note = "Use create_device_link_session_initiator (orchestrator). \
                Will be removed in Phase 3 of the device-link orchestrator rollout.")]
    pub fn send_device_link_response(
        &self,
        sender_token: String,
        encrypted_response: Vec<u8>,
    ) -> Result<(), MobileError> {
        let transport = self
            .open_vauchi_for_relay()?
            .build_relay_transport(self.relay_url.clone(), 10_000);
        device_link_relay::send_response(&transport, &sender_token, encrypted_response).map_err(
            |e| MobileError::Other {
                detail: e.to_string(),
            },
        )
    }

    /// Get the device count.
    ///
    /// Returns the number of devices linked to this identity.
    #[deprecated(note = "Use PlatformAppEngine::device_count instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn device_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;

        match storage.load_device_registry()? {
            Some(r) => Ok(r.device_count() as u32),
            None => Ok(1), // Just this device
        }
    }

    /// Unlink a device from this identity.
    ///
    /// This marks the device as revoked. It will no longer receive updates
    /// and its keys will be rotated out. Returns true if the device was
    /// found and unlinked.
    ///
    /// Note: Cannot unlink the current device (use identity deletion instead).
    /// The device_index is the position in the devices list (0-based).
    #[deprecated(note = "Use PlatformAppEngine::unlink_device instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn unlink_device(&self, device_index: u32) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;

        // Load registry
        let mut registry = match storage.load_device_registry()? {
            Some(r) => r,
            None => return Ok(false), // No registry means no other devices
        };

        // Get device at index
        let devices = registry.all_devices();
        if device_index as usize >= devices.len() {
            return Ok(false);
        }

        let device_id = devices[device_index as usize].device_id;
        let current_device_id = identity.device_info().device_id();

        // Cannot unlink current device
        if device_id == *current_device_id {
            return Err(MobileError::InvalidInput {
                field: String::new(),
                detail: "Cannot unlink the current device".to_string(),
            });
        }

        // Try to revoke the device
        match registry.revoke_device(
            &device_id,
            identity.signing_keypair(),
            storage.clock().unix_seconds(),
        ) {
            Ok(()) => {
                storage.save_device_registry(&registry)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// Check if this device is the primary device (index 0).
    #[deprecated(note = "Use PlatformAppEngine::is_primary_device instead. \
                VauchiPlatform device-linking surface will be removed in 0.40.0.")]
    pub fn is_primary_device(&self) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.device_info().device_index() == 0)
    }

    // === Multipart QR ===

    /// Encode data into multipart QR chunk strings for animated display.
    ///
    /// Each chunk fits within a QR code's data capacity. The chunks should
    /// be displayed in sequence as an animated QR code for the scanning
    /// device to reassemble using `MobileMultipartDecoder`.
    pub fn encode_multipart_qr(&self, data: Vec<u8>) -> Vec<String> {
        crate::multipart_qr::encode_multipart(&data, 1800)
    }
}

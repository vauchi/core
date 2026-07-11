// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device management operations: listing, linking, and revocation.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// Device information for display.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Index in the device list.
    pub device_index: u32,
    /// Human-readable device name.
    pub device_name: String,
    /// First 8 bytes of the public key, hex-encoded.
    pub public_key_prefix: String,
    /// Whether this is the current device.
    pub is_current: bool,
    /// Whether the device is active (not revoked).
    pub is_active: bool,
}

/// Result from generating a device link.
#[derive(Debug, Clone)]
pub struct DeviceLinkResult {
    /// ASCII art QR code for terminal display.
    pub qr_ascii: String,
    /// Data string (base64) for copy-paste transport.
    pub data_string: String,
    /// Identity fingerprint for verification.
    pub fingerprint: String,
}

impl Vauchi {
    /// Lists all linked devices.
    ///
    /// Returns device info from the registry, or falls back to the current
    /// device only if no registry exists.
    pub fn list_devices(&self) -> VauchiResult<Vec<DeviceInfo>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let current_device_id = identity.device_id();

        if let Some(registry) = self.storage.device().load_device_registry()? {
            Ok(registry
                .all_devices()
                .iter()
                .enumerate()
                .map(|(i, device)| DeviceInfo {
                    device_index: i as u32,
                    device_name: device.device_name.clone(),
                    public_key_prefix: hex::encode(&device.device_id[..8]),
                    is_current: device.device_id == *current_device_id,
                    is_active: !device.revoked,
                })
                .collect())
        } else {
            Ok(vec![DeviceInfo {
                device_index: 0,
                device_name: "This Device".to_string(),
                public_key_prefix: hex::encode(&current_device_id[..8]),
                is_current: true,
                is_active: true,
            }])
        }
    }

    /// Generates a device link QR code and data string.
    ///
    /// Returns QR ASCII art, a base64 data string, and the identity fingerprint
    /// for cross-device verification.
    pub fn generate_device_link(&self) -> VauchiResult<DeviceLinkResult> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let registry = self
            .storage
            .device()
            .load_device_registry()?
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(registry, self.clock.unix_seconds());
        let qr = initiator.qr();

        Ok(DeviceLinkResult {
            qr_ascii: qr.to_qr_image_string(),
            data_string: qr.to_data_string(),
            fingerprint: qr.identity_fingerprint(),
        })
    }

    /// Revokes a device from the registry by index.
    ///
    /// Returns the name of the revoked device. Errors if:
    /// - No identity is set
    /// - No device registry exists
    /// - The index is out of bounds
    /// - The device is the current device
    /// - The device is already revoked
    pub fn revoke_device(&self, device_index: usize) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let mut registry = self
            .storage
            .device()
            .load_device_registry()?
            .ok_or_else(|| VauchiError::InvalidState("No device registry found".into()))?;

        let devices = registry.all_devices().to_vec();
        if device_index >= devices.len() {
            return Err(VauchiError::InvalidState(format!(
                "Invalid device index: {}",
                device_index
            )));
        }

        let device = &devices[device_index];

        if device.device_id == *identity.device_id() {
            return Err(VauchiError::InvalidState(
                "Cannot revoke the current device".into(),
            ));
        }

        if device.revoked {
            return Err(VauchiError::InvalidState(format!(
                "Device '{}' is already revoked",
                device.device_name
            )));
        }

        let device_name = device.device_name.clone();
        registry
            .revoke_device(
                &device.device_id,
                identity.signing_keypair(),
                self.storage.now_secs(),
            )
            .map_err(|e| VauchiError::InvalidState(format!("Revoke failed: {:?}", e)))?;
        self.storage.device().save_device_registry(&registry)?;

        Ok(device_name)
    }

    /// Decommissions this device after a replacement handover.
    ///
    /// Wipes every contact ratchet session so this device can no longer
    /// advance a chain its replacement now owns — two devices advancing
    /// the same ratchet diverge undecryptably at the contact (ADR-035).
    /// Without sessions the send loop skips all contacts (fail-safe);
    /// incoming ratcheted updates stop decrypting as well.
    ///
    /// Returns the number of sessions wiped. Irrevocable on this device;
    /// callers must confirm with the user first (ADR-022 `InlineConfirm`).
    pub fn decommission_current_device(&self) -> VauchiResult<usize> {
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }
        Ok(self.storage.ratchets().delete_all_ratchet_states()?)
    }

    /// Returns the count of pending outbound updates across all contacts.
    pub fn pending_update_count(&self) -> VauchiResult<u32> {
        let contacts = self.storage.contacts().list_contacts()?;
        let mut total = 0u32;
        for contact in &contacts {
            let pending = self
                .storage
                .pending()
                .get_pending_updates(contact.id())
                .unwrap_or_default();
            total += pending.len() as u32;
        }
        Ok(total)
    }
}

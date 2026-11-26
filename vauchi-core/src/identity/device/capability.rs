// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{DeviceError, DeviceRegistry};
use crate::crypto::SigningKeyPair;

pub(super) fn append_device_capabilities(
    data: &mut Vec<u8>,
    signing_public_key: Option<[u8; 32]>,
    cohort: Option<u32>,
    revision: u64,
    names_keys: bool,
    names_cohorts: bool,
) {
    if names_keys || names_cohorts {
        // The marker keeps a missing key distinct from an all-zero one.
        data.push(u8::from(signing_public_key.is_some()));
        data.extend_from_slice(&signing_public_key.unwrap_or([0u8; 32]));
    }
    if names_cohorts {
        data.extend_from_slice(&revision.to_le_bytes());
        data.push(u8::from(cohort.is_some()));
        data.extend_from_slice(&cohort.unwrap_or(0).to_le_bytes());
    }
}

impl DeviceRegistry {
    pub fn set_notice_cohort(
        &mut self,
        device_id: &[u8; 32],
        cohort: Option<u32>,
        signing_key: &SigningKeyPair,
    ) -> Result<bool, DeviceError> {
        let device = self
            .devices
            .iter_mut()
            .find(|device| &device.device_id == device_id)
            .ok_or(DeviceError::DeviceNotFound)?;
        if device.revoked {
            return Err(DeviceError::SignerNotActive);
        }
        if device.notice_cohort == cohort {
            return Ok(false);
        }
        let next = device
            .notice_capability_version
            .checked_add(1)
            .ok_or(DeviceError::CapabilityVersionExhausted)?;
        let version = self
            .version
            .checked_add(1)
            .ok_or(DeviceError::CapabilityVersionExhausted)?;
        device.notice_cohort = cohort;
        device.notice_capability_version = next;
        self.version = version;
        self.sign(signing_key);
        Ok(true)
    }
}

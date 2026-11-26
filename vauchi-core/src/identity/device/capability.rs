// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{DeviceError, DeviceRegistry};
use crate::crypto::SigningKeyPair;

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

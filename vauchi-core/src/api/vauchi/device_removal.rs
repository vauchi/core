// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Removing the identity from this device while it lives on elsewhere
//! (vauchi/private#915, ADR-033 amendment 2026-10-29).

use super::super::error::VauchiResult;
use super::Vauchi;

/// What removing the identity from this device achieved; the device is
/// wiped in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRemoval {
    /// The other devices were sent this device's revocation.
    OtherDevicesTold,
    /// The revocation could not be sent (offline); remove this device
    /// from one of the other devices.
    OtherDevicesNotTold,
    /// Duress mode: the emergency wipe ran and nothing was sent.
    Wiped,
}

impl Vauchi {
    /// Revokes this device, sends that to the identity's other devices in
    /// one sync, then wipes this device; contacts and the relay are not
    /// told. In duress mode it is the emergency wipe and sends nothing.
    ///
    /// The sync runs outside the timing gate (maintainer decision
    /// 2026-10-29): the revocation must leave before the wipe destroys the
    /// queue, and the device is gone right after.
    ///
    /// Errors, before wiping anything, if no identity is set or this is the
    /// identity's last active device (that is Leave Vauchi, #916).
    pub fn remove_identity_from_this_device(&mut self) -> VauchiResult<DeviceRemoval> {
        if self.in_duress_mode() {
            self.perform_emergency_wipe(true)?;
            return Ok(DeviceRemoval::Wiped);
        }
        self.revoke_this_device()?;
        let told = self.sync_once_outside_the_gate();
        self.perform_emergency_wipe(true)?;
        Ok(if told {
            DeviceRemoval::OtherDevicesTold
        } else {
            DeviceRemoval::OtherDevicesNotTold
        })
    }

    #[cfg(feature = "network-http")]
    fn sync_once_outside_the_gate(&mut self) -> bool {
        if !self.has_ohttp_key() && self.connect().is_err() {
            return false;
        }
        self.next_sync_allowed = None;
        matches!(
            self.sync(),
            Ok(super::VauchiSyncOutcome::Ok { ref errors, .. }) if errors.is_empty()
        )
    }

    #[cfg(not(feature = "network-http"))]
    fn sync_once_outside_the_gate(&mut self) -> bool {
        false
    }
}

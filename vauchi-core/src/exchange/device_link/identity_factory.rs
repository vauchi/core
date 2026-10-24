// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::identity::{DeviceRegistry, Identity};

impl Identity {
    /// Creates a device link initiator for linking a new device.
    ///
    /// This generates a QR code that can be scanned by a new device
    /// to receive the identity's master seed.
    pub fn create_device_link_initiator(
        &self,
        registry: DeviceRegistry,
        now: u64,
    ) -> crate::exchange::DeviceLinkInitiator {
        crate::exchange::DeviceLinkInitiator::new(*self.master_seed(), self, registry, now)
    }

    /// Restores a device link initiator from a saved QR code.
    ///
    /// Used when the QR was generated earlier and saved, then the
    /// request comes in later.
    pub fn restore_device_link_initiator(
        &self,
        registry: DeviceRegistry,
        qr: crate::exchange::DeviceLinkQR,
    ) -> crate::exchange::DeviceLinkInitiatorRestored {
        crate::exchange::DeviceLinkInitiatorRestored::new(*self.master_seed(), self, registry, qr)
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a device does with its own identity's registry from a sibling.
//!
//! Devices are never removed from a registry, only revoked, so two
//! siblings' registries merge as a union: every device, every key a device
//! named, every revocation. The merge is the same whichever sibling makes
//! it, so two siblings merging at once publish the same registry, signed
//! the same, and converge (private#970).

use std::cmp::Ordering;

use super::{DeviceError, DeviceRegistry, MAX_DEVICES, RegisteredDevice};
use crate::crypto::SigningKeyPair;

/// A device's answer to its own identity's registry from a sibling.
#[derive(Debug, Clone)]
pub enum Reconciled {
    /// Keep the registry this device holds: it has everything.
    Keep,
    /// Save the sibling's registry as it is: it has everything and is newer.
    Adopt,
    /// Save this merge of the two, and send it to the siblings.
    Publish(DeviceRegistry),
}

impl DeviceRegistry {
    /// Decides what to do with `incoming`, a sibling's registry of this
    /// identity whose signature the caller verified. A merge that differs
    /// from both is published one version above the higher of the two.
    ///
    /// # Errors
    /// [`DeviceError::MaxDevicesReached`] when the union holds more devices
    /// than a registry may.
    pub fn reconcile(
        &self,
        incoming: &DeviceRegistry,
        signing_key: &SigningKeyPair,
    ) -> Result<Reconciled, DeviceError> {
        let merged = self.merged_devices(incoming)?;
        if same_signed_devices(&merged, &incoming.devices) && incoming.version > self.version {
            return Ok(Reconciled::Adopt);
        }
        if same_signed_devices(&merged, &self.devices) && self.version >= incoming.version {
            return Ok(Reconciled::Keep);
        }
        let mut published = DeviceRegistry {
            devices: merged,
            version: self.version.max(incoming.version).saturating_add(1),
            signature: [0u8; 64],
        };
        published.sign(signing_key);
        Ok(Reconciled::Publish(published))
    }

    /// The union, in the order of the newer registry (the one with the
    /// smaller signature at equal versions), which keeps its primary;
    /// devices only the other holds follow, by creation time and id.
    fn merged_devices(
        &self,
        incoming: &DeviceRegistry,
    ) -> Result<Vec<RegisteredDevice>, DeviceError> {
        let (base, other) = match self.version.cmp(&incoming.version) {
            Ordering::Greater => (self, incoming),
            Ordering::Less => (incoming, self),
            Ordering::Equal if self.signature <= incoming.signature => (self, incoming),
            Ordering::Equal => (incoming, self),
        };
        let mut merged: Vec<RegisteredDevice> = base
            .devices
            .iter()
            .map(|device| match other.find_device(&device.device_id) {
                Some(twin) => merged_entry(device, twin),
                None => device.clone(),
            })
            .collect();
        let mut only_other: Vec<RegisteredDevice> = other
            .devices
            .iter()
            .filter(|device| base.find_device(&device.device_id).is_none())
            .cloned()
            .collect();
        only_other.sort_by_key(|device| (device.created_at, device.device_id));
        merged.extend(only_other);
        if merged.len() > MAX_DEVICES {
            return Err(DeviceError::MaxDevicesReached);
        }
        Ok(merged)
    }
}

/// One device's entry from both registries: a key either names (the
/// smaller if both name different ones, so either order agrees), revoked
/// if either revoked it.
fn merged_entry(base: &RegisteredDevice, other: &RegisteredDevice) -> RegisteredDevice {
    let either = |a: Option<[u8; 32]>, b: Option<[u8; 32]>| match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    RegisteredDevice {
        exchange_public_key: base.exchange_public_key.min(other.exchange_public_key),
        signing_public_key: either(base.signing_public_key, other.signing_public_key),
        revoked: base.revoked || other.revoked,
        revoked_at: match (base.revoked_at, other.revoked_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        },
        created_at: base.created_at.min(other.created_at),
        last_sync_at: base.last_sync_at.max(other.last_sync_at),
        ..base.clone()
    }
}

/// Whether two device lists sign the same: id, exchange key, signing key
/// and revocation, in order.
fn same_signed_devices(a: &[RegisteredDevice], b: &[RegisteredDevice]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.device_id == y.device_id
                && x.exchange_public_key == y.exchange_public_key
                && x.signing_public_key == y.signing_public_key
                && x.revoked == y.revoked
        })
}

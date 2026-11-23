// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a device does with its own identity's registry from a sibling.

use super::{DeviceError, DeviceRegistry};
use crate::crypto::SigningKeyPair;

/// A device's answer to its own identity's registry from a sibling.
#[derive(Debug, Clone)]
pub enum Reconciled {
    /// Keep the registry this device holds.
    Keep,
    /// Save the sibling's registry as it is.
    Adopt,
    /// Save this registry, and send it to the siblings.
    Publish(DeviceRegistry),
}

impl DeviceRegistry {
    /// Decides what to do with `incoming`, a sibling's registry of this
    /// identity whose signature the caller verified.
    ///
    /// # Errors
    /// None yet; the signature keeps the rule's shape.
    pub fn reconcile(
        &self,
        incoming: &DeviceRegistry,
        _signing_key: &SigningKeyPair,
    ) -> Result<Reconciled, DeviceError> {
        if incoming.version > self.version {
            Ok(Reconciled::Adopt)
        } else {
            Ok(Reconciled::Keep)
        }
    }
}

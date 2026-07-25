// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 sibling-relay apply arms (ADR-064 Amendment 2026-07-25).
//!
//! Bodies for the `ContactRegistryReceived` / `ContactActivationChanged`
//! sync items — kept out of `propagation.rs`'s apply match for file-size
//! reasons and so their failure mode is uniform: every rejection is a
//! per-item `Err` value that the tolerant apply loop skips, never a
//! batch-aborting early return.

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::sync::registry_activation::ActivationTracker;

impl Vauchi {
    /// Persist a relayed contact registry on this sibling.
    ///
    /// Same guard as direct receipt: signature re-verified against the
    /// contact's pinned key, monotonic version, unbounded age (relay delay
    /// is unbounded). A version we already hold (or newer) is an idempotent
    /// no-op; an owner-removed contact stays removed.
    pub(super) fn apply_contact_registry_received(
        &self,
        contact_id: &str,
        registry_json: &str,
    ) -> VauchiResult<()> {
        let Some(contact) = self.storage.contacts().load_contact(contact_id)? else {
            return Ok(());
        };
        let broadcast = crate::identity::RegistryBroadcast::from_json(registry_json)
            .map_err(|error| VauchiError::InvalidState(error.to_string()))?;
        let contact_pk = contact
            .public_key()
            .ok_or_else(|| VauchiError::InvalidState("contact has no identity key".into()))?;
        match self.storage.device().save_contact_device_registry(
            contact_id,
            &broadcast,
            contact_pk,
            u64::MAX,
        ) {
            Ok(()) => Ok(()),
            Err(save_error) => {
                let held = self
                    .storage
                    .device()
                    .load_contact_device_registry(contact_id)?;
                match held {
                    Some(stored) if stored.version() >= broadcast.version() => Ok(()),
                    _ => Err(VauchiError::InvalidState(format!(
                        "relayed contact registry rejected: {save_error}"
                    ))),
                }
            }
        }
    }

    /// Merge a relayed activation snapshot into this sibling's tracker.
    pub(super) fn apply_contact_activation_changed(
        &self,
        contact_id: &str,
        push_nonce: &Option<Vec<u8>>,
        pushed_version: Option<u64>,
        our_version_acked: Option<u64>,
        peer_version_held: Option<u64>,
    ) -> VauchiResult<()> {
        if self.storage.contacts().load_contact(contact_id)?.is_none() {
            return Ok(());
        }
        let outstanding = match (push_nonce, pushed_version) {
            (Some(nonce_bytes), Some(version)) => {
                let nonce: [u8; 32] = nonce_bytes.clone().try_into().map_err(|_| {
                    VauchiError::InvalidState("relayed activation nonce length".into())
                })?;
                Some((nonce, version))
            }
            (None, None) => None,
            _ => {
                return Err(VauchiError::InvalidState(
                    "relayed activation push fields disagree".into(),
                ));
            }
        };
        let incoming =
            ActivationTracker::from_parts(outstanding, our_version_acked, peer_version_held);
        let mut tracker = self
            .storage
            .registry_activation()
            .load_activation(contact_id)?
            .unwrap_or_default();
        tracker.merge_snapshot(&incoming);
        self.storage
            .registry_activation()
            .save_activation(contact_id, &tracker)
            .map_err(|e| e.into())
    }
}

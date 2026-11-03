// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A recovered identity's unfinished rekeys in the device-link full sync
//! (vauchi/private#796, D26): a device linked while contacts still wait
//! takes over the waiting list, each offer's exchange key and the proof, so
//! it can offer and complete too. The old identity's seed stays on the
//! device that restored it: old-key signatures may never authorize a
//! recovery or a merge on their own (owner decision 2026-11-03, social
//! recovery security review).

use crate::storage::{Storage, StorageError};
use crate::sync::device_sync::{DeviceSyncError, RecoveryRekeySyncData, RekeyWaitingSyncData};

/// The rekey state to hand a newly linked device; `None` when no contact
/// waits.
pub(super) fn rekey_snapshot(
    storage: &Storage,
) -> Result<Option<RecoveryRekeySyncData>, StorageError> {
    let recovery = storage.recovery();
    let pending = recovery.rekey_pending_since()?;
    if pending.is_empty() {
        return Ok(None);
    }
    let mut waiting = Vec::with_capacity(pending.len());
    for (contact_id, restored_at) in pending {
        let exchange = recovery.rekey_exchange(&contact_id)?;
        waiting.push(RekeyWaitingSyncData {
            met_again: recovery.rekey_met_again(&contact_id)?,
            exchange_secret: exchange.as_ref().map(|(secret, _)| secret.to_vec()),
            offer_sent_at: exchange.and_then(|(_, sent_at)| sent_at),
            contact_id,
            restored_at,
        });
    }
    Ok(Some(RecoveryRekeySyncData {
        waiting,
        own_proof: recovery.load_own_proof()?,
    }))
}

/// Takes over the rekey state a linked device sent. A waiting contact
/// this device does not hold is skipped.
///
/// # Errors
/// [`DeviceSyncError::Deserialization`] for an exchange key that is not 32
/// bytes, or a proof over the offer's bound; storage errors.
pub(super) fn restore_rekey_snapshot(
    storage: &Storage,
    snapshot: &RecoveryRekeySyncData,
) -> Result<(), DeviceSyncError> {
    let stored = |e: StorageError| DeviceSyncError::Deserialization(e.to_string());
    let recovery = storage.recovery();
    if let Some(proof) = &snapshot.own_proof {
        if proof.len() > crate::recovery::rekey::MAX_PROOF_BYTES {
            return Err(DeviceSyncError::Deserialization(
                "recovery proof over its bound".into(),
            ));
        }
        recovery.save_own_proof(proof).map_err(stored)?;
    }
    for waiting in &snapshot.waiting {
        if storage
            .contacts()
            .load_contact(&waiting.contact_id)
            .map_err(stored)?
            .is_none()
        {
            continue;
        }
        recovery
            .restore_rekey_pending(&waiting.contact_id, waiting.restored_at, waiting.met_again)
            .map_err(stored)?;
        if let Some(secret) = &waiting.exchange_secret {
            recovery
                .save_rekey_exchange(&waiting.contact_id, &*thirty_two(secret, "exchange key")?)
                .map_err(stored)?;
        }
        if let Some(at) = waiting.offer_sent_at {
            recovery
                .record_rekey_offer_sent(&waiting.contact_id, at)
                .map_err(stored)?;
        }
    }
    Ok(())
}

fn thirty_two(bytes: &[u8], what: &str) -> Result<zeroize::Zeroizing<[u8; 32]>, DeviceSyncError> {
    <[u8; 32]>::try_from(bytes)
        .map(zeroize::Zeroizing::new)
        .map_err(|_| DeviceSyncError::Deserialization(format!("{what} is not 32 bytes")))
}

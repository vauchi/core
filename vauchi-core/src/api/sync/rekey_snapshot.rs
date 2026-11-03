// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A recovered identity's unfinished rekeys in the device-link full sync
//! (vauchi/private#796, D26): a device linked while contacts still wait
//! takes over the waiting list, each offer's exchange key, the proof and
//! the old seed, so it can offer, complete and sign continuations too.

use crate::storage::{Storage, StorageError};
use crate::sync::device_sync::{DeviceSyncError, RecoveryRekeySyncData, RekeyWaitingSyncData};

/// The rekey state to hand a newly linked device; `None` when this
/// identity was never restored from a guardian backup.
pub(super) fn rekey_snapshot(
    storage: &Storage,
) -> Result<Option<RecoveryRekeySyncData>, StorageError> {
    let recovery = storage.recovery();
    let old_seed = recovery.load_old_identity_seed()?;
    let pending = recovery.rekey_pending_since()?;
    if old_seed.is_none() && pending.is_empty() {
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
        old_seed: old_seed.map(|seed| seed.to_vec()),
    }))
}

/// Takes over the rekey state a linked device sent. A waiting contact
/// this device does not hold is skipped.
///
/// # Errors
/// [`DeviceSyncError::Deserialization`] for a seed or exchange key that is
/// not 32 bytes, or a proof over the offer's bound; storage errors.
pub(super) fn restore_rekey_snapshot(
    storage: &Storage,
    snapshot: &RecoveryRekeySyncData,
) -> Result<(), DeviceSyncError> {
    let stored = |e: StorageError| DeviceSyncError::Deserialization(e.to_string());
    let recovery = storage.recovery();
    if let Some(seed) = &snapshot.old_seed {
        recovery
            .save_old_identity_seed(&*thirty_two(seed, "old identity seed")?)
            .map_err(stored)?;
    }
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

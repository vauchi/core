// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sync item that carries a recovered contact's rekey to this
//! identity's other devices (vauchi/private#796, D25), and its journal
//! from the receive path, where no `Vauchi` is at hand.

use crate::identity::Identity;
use crate::storage::{Storage, StorageError};
use crate::sync::device_sync::{ContactRekeySyncData, SyncItem};

/// The item telling the siblings that the contact `old_id` is now
/// `new_id`; `None` when `new_id` is not an exchanged contact.
pub(crate) fn contact_rekeyed_item(
    storage: &Storage,
    old_id: &str,
    new_id: &str,
) -> Result<Option<SyncItem>, StorageError> {
    let Some(contact) = storage.contacts().load_contact(new_id)? else {
        return Ok(None);
    };
    let (Some(public_key), Some(shared_key)) = (contact.public_key(), contact.shared_key()) else {
        return Ok(None);
    };
    Ok(Some(SyncItem::ContactRekeyed {
        rekey: ContactRekeySyncData {
            old_contact_id: old_id.to_string(),
            new_public_key: (*public_key).into(),
            shared_key: *shared_key.as_bytes(),
        },
        timestamp: storage.clock().unix_seconds(),
    }))
}

/// Journals the rekey for the siblings after the receive transaction
/// committed (`record_local_change` opens its own, the same seam as
/// `journal_handshake_state_for_siblings`). Best effort: a lost journal
/// leaves a sibling on the old key until the device-link full sync.
pub(crate) fn journal_contact_rekeyed_for_siblings(
    identity: &Identity,
    storage: &Storage,
    old_id: &str,
    new_id: &str,
) {
    if let Err(error) = try_journal(identity, storage, old_id, new_id) {
        tracing::warn!("sibling rekey journal failed: {error}");
    }
}

fn try_journal(
    identity: &Identity,
    storage: &Storage,
    old_id: &str,
    new_id: &str,
) -> Result<(), String> {
    let Some(own_registry) = storage
        .device()
        .load_device_registry()
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if own_registry.device_count() <= 1 {
        return Ok(());
    }
    let Some(item) = contact_rekeyed_item(storage, old_id, new_id).map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    super::device_orchestrator::DeviceSyncOrchestrator::load(
        storage,
        identity.create_device_info(storage.clock().unix_seconds()),
        own_registry,
    )
    .map_err(|e| e.to_string())?
    .record_local_change(item)
    .map_err(|e| e.to_string())
}

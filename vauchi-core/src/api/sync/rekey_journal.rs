// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sync item that carries a recovered contact's rekey to this
//! identity's other devices (vauchi/private#796, D25).

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

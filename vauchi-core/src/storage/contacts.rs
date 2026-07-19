// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact storage forwarders to [`ContactStore`](super::ContactStore); plus the cross-cutting `delete_contact` orchestrator.

use super::{Storage, StorageError};
use rusqlite::params;

impl Storage {
    /// Deletes a contact by ID.
    pub fn delete_contact(&self, id: &str) -> Result<bool, StorageError> {
        // Clear relationship-scoped state that neither lives on the contacts
        // row (nickname/avatar/notes/cek drop with the row) nor cascades via
        // FK (contact_field_notes/contact_shared_names/contact_shared_avatars).
        // Without this these rows orphan; a stale contact_sync_timestamps row
        // in particular wrongly gates sync on contact_id reuse (read with
        // `.unwrap_or(0)` in sync/state.rs). See problem
        // 2026-06-01-contact-delete-orphans.
        self.conn.execute(
            "DELETE FROM contact_ratchets WHERE contact_id = ?1",
            params![id],
        )?;
        self.conn.execute(
            "DELETE FROM contact_device_delta_versions WHERE contact_id = ?1",
            params![id],
        )?;
        self.sync().forget_contact(id)?;
        self.pending().delete_pending_updates_for_contact(id)?;
        self.labels().delete_all_contact_overrides(id)?;
        self.conn.execute(
            "DELETE FROM dismissed_duplicates WHERE id1 = ?1 OR id2 = ?1",
            params![id],
        )?;

        let rows_affected = self
            .conn
            .execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }
}

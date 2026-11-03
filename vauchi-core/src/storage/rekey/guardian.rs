// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rekey of the guardian designator tags (vauchi/private#796, D15).

use super::*;
use crate::storage::stores::designator_tag;

impl Storage {
    /// Moves every guardian designator tag from its old-key value to its
    /// new-key one, through the contacts the tags were made from. The rows'
    /// encrypted columns are rekeyed by their own steps.
    pub(super) fn retag_guardian_designators(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let ids: Vec<String> = self
            .conn
            .prepare("SELECT id FROM contacts")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| StorageError::Migration(format!("Read contact ids: {e}")))?;
        for id in ids {
            let (old_tag, new_tag) = (designator_tag(old_key, &id), designator_tag(new_key, &id));
            for table in ["guardian_backup_pieces", "guardian_backups"] {
                self.conn
                    .execute(
                        &format!(
                            "UPDATE {table} SET designator_tag = ?1 WHERE designator_tag = ?2"
                        ),
                        params![new_tag, old_tag],
                    )
                    .map_err(|e| StorageError::Migration(format!("Retag {table}: {e}")))?;
            }
        }
        Ok(())
    }
}

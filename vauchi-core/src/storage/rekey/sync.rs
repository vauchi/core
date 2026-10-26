// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rekey of sync state: version vectors, timestamps, queues, checkpoints.

use super::*;

impl Storage {
    /// Re-encrypt device_sync_state: state_json_encrypted
    pub(super) fn rekey_device_sync_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT device_id, state_json_encrypted FROM device_sync_state WHERE state_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read device_sync: {}", e)))?;

        let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query device_sync: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect device_sync: {}", e)))?;

        for (device_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt device_sync: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt device_sync: {}", e)))?;
                self.conn.execute(
                    "UPDATE device_sync_state SET state_json_encrypted = ?1 WHERE device_id = ?2",
                    params![new_enc, device_id],
                ).map_err(|e| StorageError::Migration(format!("Update device_sync: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt version_vector: vector_json_encrypted
    /// Re-encrypt receive_checkpoints: checkpoint_json_encrypted, one row
    /// per relay/identity/device scope (vauchi/private#691).
    pub(super) fn rekey_receive_checkpoints(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT scope, checkpoint_json_encrypted FROM receive_checkpoints")
            .map_err(|e| StorageError::Migration(format!("Read receive_checkpoints: {e}")))?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query receive_checkpoints: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect receive_checkpoints: {e}")))?;
        for (scope, enc) in &rows {
            let plain = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt receive_checkpoint: {e}")))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt receive_checkpoint: {e}")))?;
            self.conn
                .execute(
                    "UPDATE receive_checkpoints SET checkpoint_json_encrypted = ?1 WHERE scope = ?2",
                    params![new_enc, scope],
                )
                .map_err(|e| StorageError::Migration(format!("Update receive_checkpoint: {e}")))?;
        }
        Ok(())
    }

    pub(super) fn rekey_version_vector(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT vector_json_encrypted FROM version_vector WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt version_vector: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt version_vector: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE version_vector SET vector_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update version_vector: {}", e)))?;
        }
        Ok(())
    }

    pub(super) fn rekey_field_timestamps(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT timestamps_json_encrypted FROM sync_field_timestamps WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt sync_field_timestamps: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt sync_field_timestamps: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE sync_field_timestamps SET timestamps_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| {
                    StorageError::Migration(format!("Update sync_field_timestamps: {}", e))
                })?;
        }
        Ok(())
    }

    /// Re-encrypt contact_sync_timestamps: last_sync_at_encrypted
    pub(super) fn rekey_sync_timestamps(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT contact_id, last_sync_at_encrypted FROM contact_sync_timestamps WHERE last_sync_at_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read sync_timestamps: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query sync_timestamps: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect sync_timestamps: {}", e)))?;

        for (contact_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt sync_ts: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt sync_ts: {}", e)))?;
                self.conn.execute(
                    "UPDATE contact_sync_timestamps SET last_sync_at_encrypted = ?1 WHERE contact_id = ?2",
                    params![new_enc, contact_id],
                ).map_err(|e| StorageError::Migration(format!("Update sync_ts: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt pending_updates: payload_encrypted
    pub(super) fn rekey_pending_updates(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT id, payload_encrypted FROM pending_updates WHERE payload_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read pending: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query pending: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect pending: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt pending {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt pending {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE pending_updates SET payload_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update pending {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt retry_entries: payload_encrypted
    pub(super) fn rekey_retry_entries(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT message_id, payload_encrypted FROM retry_entries WHERE payload_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read retry: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query retry: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect retry: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt retry {}: {}", id, e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt retry {}: {}", id, e)))?;
                self.conn
                    .execute(
                        "UPDATE retry_entries SET payload_encrypted = ?1 WHERE message_id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update retry {}: {}", id, e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt device_sync_checkpoints: items_json_encrypted
    pub(super) fn rekey_device_sync_checkpoints(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT target_device_id, items_json_encrypted FROM device_sync_checkpoints WHERE items_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read checkpoints: {}", e)))?;

        let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect checkpoints: {}", e)))?;

        for (device_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt checkpoint: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt checkpoint: {}", e)))?;
                self.conn.execute(
                    "UPDATE device_sync_checkpoints SET items_json_encrypted = ?1 WHERE target_device_id = ?2",
                    params![new_enc, device_id],
                ).map_err(|e| StorageError::Migration(format!("Update checkpoint: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt sync_checkpoints: state_json_encrypted
    pub(super) fn rekey_sync_checkpoints(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT checkpoint_id, state_json_encrypted FROM sync_checkpoints WHERE state_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read batch_checkpoints: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query batch_checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect batch_checkpoints: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt batch_checkpoint {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt batch_checkpoint {}: {}", id, e))
                })?;
                self.conn.execute(
                    "UPDATE sync_checkpoints SET state_json_encrypted = ?1 WHERE checkpoint_id = ?2",
                    params![new_enc, id],
                ).map_err(|e| StorageError::Migration(format!("Update batch_checkpoint {}: {}", id, e)))?;
            }
        }
        Ok(())
    }
}

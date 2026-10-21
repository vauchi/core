// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rekey of the identity, its devices, and their secrets.

use super::*;

impl Storage {
    /// Re-encrypt identity: backup_data_encrypted
    pub(super) fn rekey_identity_backup(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(i64, Vec<u8>), _> = self.conn.query_row(
            "SELECT id, backup_data_encrypted FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((id, backup_enc)) = result {
            let plain = decrypt(old_key, &backup_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt identity: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt identity: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE identity SET backup_data_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update identity: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt ratchet state with per-contact derived keys (#126)
    pub(super) fn rekey_ratchets(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT contact_id, peer_device_id, ratchet_state_encrypted
                 FROM contact_ratchets",
            )
            .map_err(|e| StorageError::Migration(format!("Read ratchets: {}", e)))?;

        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query ratchets: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect ratchets: {}", e)))?;

        for (contact_id, peer_device_id, ratchet_enc) in &rows {
            // Decrypt with old per-contact key
            let mut old_info = b"vauchi-ratchet-storage-v1:".to_vec();
            old_info.extend_from_slice(contact_id.as_bytes());
            let old_derived = HKDF::derive_key(None, old_key.as_bytes(), &old_info);
            let old_ratchet_key = SymmetricKey::from_bytes(*old_derived);

            let plain = decrypt(&old_ratchet_key, ratchet_enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt ratchet {}: {}", contact_id, e))
            })?;

            // Re-encrypt with new per-contact key
            let mut new_info = b"vauchi-ratchet-storage-v1:".to_vec();
            new_info.extend_from_slice(contact_id.as_bytes());
            let new_derived = HKDF::derive_key(None, new_key.as_bytes(), &new_info);
            let new_ratchet_key = SymmetricKey::from_bytes(*new_derived);

            let new_enc = encrypt(&new_ratchet_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt ratchet {}: {}", contact_id, e))
            })?;
            self.conn
                .execute(
                    "UPDATE contact_ratchets
                 SET ratchet_state_encrypted = ?1
                 WHERE contact_id = ?2 AND peer_device_id = ?3",
                    params![new_enc, contact_id, peer_device_id],
                )
                .map_err(|e| {
                    StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e))
                })?;
        }
        Ok(())
    }

    /// Re-encrypt own_card: card_json_encrypted
    pub(super) fn rekey_own_card(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT card_json_encrypted FROM own_card WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt own_card: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt own_card: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE own_card SET card_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update own_card: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt device_registry: registry_json_encrypted
    pub(super) fn rekey_device_registry(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT registry_json_encrypted FROM device_registry WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt registry: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt registry: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE device_registry SET registry_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update registry: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt every retained contact device-registry broadcast.
    pub(super) fn rekey_contact_device_registries(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, broadcast_encrypted FROM contact_device_registries")
            .map_err(|e| StorageError::Migration(format!("Read contact registries: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|e| StorageError::Migration(format!("Query contact registries: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact registries: {}", e)))?;
        drop(stmt);

        for (contact_id, encrypted) in rows {
            let plain = decrypt(old_key, &encrypted).map_err(|e| {
                StorageError::Migration(format!("Decrypt contact registry {}: {}", contact_id, e))
            })?;
            let reencrypted = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt contact registry {}: {}", contact_id, e))
            })?;
            self.conn
                .execute(
                    "UPDATE contact_device_registries SET broadcast_encrypted = ?1 WHERE contact_id = ?2",
                    params![reencrypted, contact_id],
                )
                .map_err(|e| StorageError::Migration(format!("Update contact registry: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt device_info: device_info_encrypted
    pub(super) fn rekey_device_info(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT device_info_encrypted FROM device_info WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt device_info: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt device_info: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE device_info SET device_info_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update device_info: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt identity: password_hash_encrypted, duress_hash_encrypted (nullable)
    pub(super) fn rekey_identity_passwords(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        #[allow(clippy::type_complexity)]
        let result: Result<(i64, Option<Vec<u8>>, Option<Vec<u8>>), _> = self.conn.query_row(
            "SELECT id, password_hash_encrypted, duress_hash_encrypted FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        );

        if let Ok((id, pw_enc, duress_enc)) = result {
            if let Some(enc) = pw_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt password_hash: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt password_hash: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE identity SET password_hash_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update password_hash: {}", e)))?;
            }
            if let Some(enc) = duress_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt duress_hash: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt duress_hash: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE identity SET duress_hash_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update duress_hash: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt ux_state: aha_tracker, demo_contact, onboarding_progress,
    /// backup_reminder, settings_flags
    /// Re-encrypts the user-presence digest (#712); kept apart from
    /// `rekey_ux_state`'s one-row tuple so that tuple does not grow again.
    pub(super) fn rekey_user_presence_digest(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result = self.conn.query_row(
            "SELECT id, user_presence_digest_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
        );
        let (id, enc) = match result {
            Ok((id, Some(enc))) if !enc.is_empty() => (id, enc),
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
            Err(e) => return Err(StorageError::Database(e)),
        };
        let plain = decrypt(old_key, &enc)
            .map_err(|e| StorageError::Migration(format!("Decrypt user_presence_digest: {}", e)))?;
        let new_enc = encrypt(new_key, &plain)
            .map_err(|e| StorageError::Migration(format!("Encrypt user_presence_digest: {}", e)))?;
        self.conn
            .execute(
                "UPDATE ux_state SET user_presence_digest_encrypted = ?1 WHERE id = ?2",
                params![new_enc, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update user_presence_digest: {}", e)))?;
        Ok(())
    }
}

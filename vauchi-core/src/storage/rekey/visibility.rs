// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rekey of visibility labels and field validations.

use super::*;

impl Storage {
    /// Re-encrypt visibility_labels: contacts_json, visible_fields_json, name, name_hmac, display_name_override, bio_override, avatar_override
    pub(super) fn rekey_visibility_labels(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
        table: &str,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id, contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted, display_name_override_encrypted, bio_override_encrypted, avatar_override_encrypted FROM {table} WHERE contacts_json_encrypted IS NOT NULL"))
            .map_err(|e| StorageError::Migration(format!("Read labels: {}", e)))?;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            Vec<u8>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(|e| StorageError::Migration(format!("Query labels: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect labels: {}", e)))?;

        // Derive HMAC keys for old and new SEK
        let new_hmac_key_bytes =
            HKDF::derive_key(None, new_key.as_bytes(), b"Vauchi_Label_Name_HMAC_v1");
        let new_hmac_key_ref: &[u8] = &*new_hmac_key_bytes;

        for (
            id,
            contacts_enc,
            fields_enc,
            name_enc,
            override_enc,
            bio_override_enc,
            avatar_override_enc,
        ) in &rows
        {
            let contacts_new = if !contacts_enc.is_empty() {
                let plain = decrypt(old_key, contacts_enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt label contacts {}: {}", id, e))
                })?;
                encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt label contacts {}: {}", id, e))
                })?
            } else {
                contacts_enc.clone()
            };

            let fields_new = if let Some(enc) = fields_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label fields {}: {}", id, e))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label fields {}: {}", id, e))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            // Re-encrypt name and recompute HMAC (#128)
            let (name_new, name_hmac_new) = if let Some(enc) = name_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label name {}: {}", id, e))
                    })?;
                    let new_enc = encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label name {}: {}", id, e))
                    })?;
                    let mut mac = HmacSha256::new_from_slice(new_hmac_key_ref)
                        .expect("HMAC accepts any key length");
                    mac.update(&plain);
                    let hmac_val = mac.finalize().into_bytes();
                    (Some(new_enc), Some(hmac_val.to_vec()))
                } else {
                    (Some(enc.clone()), None)
                }
            } else {
                (None, None)
            };

            let override_new = if let Some(enc) = override_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label override: {}", e))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label override: {}", e))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            let bio_override_new = if let Some(enc) = bio_override_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label bio override {}: {}", id, e))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label bio override {}: {}", id, e))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            let avatar_override_new = if let Some(enc) = avatar_override_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!(
                            "Decrypt label avatar override {}: {}",
                            id, e
                        ))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!(
                            "Encrypt label avatar override {}: {}",
                            id, e
                        ))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            self.conn.execute(
                &format!("UPDATE {table} SET contacts_json_encrypted = ?1, visible_fields_json_encrypted = ?2, name_encrypted = ?3, name_hmac = ?4, display_name_override_encrypted = ?5, bio_override_encrypted = ?6, avatar_override_encrypted = ?7 WHERE id = ?8"),
                params![contacts_new, fields_new, name_new, name_hmac_new, override_new, bio_override_new, avatar_override_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update label: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt field_validations: field_value_encrypted, signature_encrypted
    pub(super) fn rekey_field_validations(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, field_value_encrypted, signature_encrypted FROM field_validations")
            .map_err(|e| {
                StorageError::Migration(format!("Read field_validations for rekey: {}", e))
            })?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query field_validations: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect field_validations: {}", e)))?;

        for (id, fv_enc, sig_enc) in &rows {
            if let Some(enc) = fv_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt field_value {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt field_value {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE field_validations SET field_value_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update field_value {}: {}", id, e))
                    })?;
            }
            if let Some(enc) = sig_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt signature {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt signature {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE field_validations SET signature_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update signature {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }
}

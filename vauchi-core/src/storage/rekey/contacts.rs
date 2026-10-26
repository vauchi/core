// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rekey of contact rows and the tables keyed by a contact.

use super::*;

impl Storage {
    /// Re-encrypt contacts: card_encrypted and shared_key_encrypted.
    ///
    /// A card under a CEK stays as it is (its CEK is re-wrapped with
    /// `cek_encrypted`), and an imported contact has no shared key to
    /// re-encrypt (#585).
    pub(super) fn rekey_contacts(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, card_encrypted, shared_key_encrypted, cek_encrypted IS NOT NULL FROM contacts")
            .map_err(|e| StorageError::Migration(format!("Read contacts: {}", e)))?;

        let rows: Vec<(String, Vec<u8>, Vec<u8>, bool)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| StorageError::Migration(format!("Query contacts: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contacts: {}", e)))?;

        for (id, card_enc, key_enc, card_under_cek) in &rows {
            let card_new = if *card_under_cek {
                card_enc.clone()
            } else {
                let card_plain = decrypt(old_key, card_enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt card {}: {}", id, e)))?;
                encrypt(new_key, &card_plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt card {}: {}", id, e)))?
            };
            let key_new = if key_enc.is_empty() {
                Vec::new()
            } else {
                let key_plain = decrypt(old_key, key_enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt key {}: {}", id, e)))?;
                encrypt(new_key, &key_plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt key {}: {}", id, e)))?
            };

            self.conn.execute(
                "UPDATE contacts SET card_encrypted = ?1, shared_key_encrypted = ?2 WHERE id = ?3",
                params![card_new, key_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contacts: personal_notes_encrypted and avatar_encrypted (nullable)
    ///
    /// Self-healing: these columns may contain plaintext (pre-encryption gap from
    /// migration V4). Valid ciphertexts start with 0x02/0x03 algorithm tags; UTF-8
    /// text never does. If decrypt fails, the data is plaintext — encrypt it
    /// directly with the new key, healing the gap in-place.
    pub(super) fn rekey_contact_extras(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, personal_notes_encrypted, avatar_encrypted FROM contacts WHERE personal_notes_encrypted IS NOT NULL OR avatar_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact extras: {}", e)))?;

        type ContactExtras = (String, Option<Vec<u8>>, Option<Vec<u8>>);
        let rows: Vec<ContactExtras> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact extras: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact extras: {}", e)))?;

        for (id, notes_enc, avatar_enc) in &rows {
            let notes_new =
                if let Some(enc) = notes_enc {
                    Some(rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Rekey notes {}: {}", id, e))
                    })?)
                } else {
                    None
                };

            let avatar_new =
                if let Some(enc) = avatar_enc {
                    Some(rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Rekey avatar {}: {}", id, e))
                    })?)
                } else {
                    None
                };

            self.conn.execute(
                "UPDATE contacts SET personal_notes_encrypted = ?1, avatar_encrypted = ?2 WHERE id = ?3",
                params![notes_new, avatar_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact extras {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contact_field_notes: note_encrypted (self-healing, same gap as above)
    pub(super) fn rekey_contact_field_notes(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, field_id, note_encrypted FROM contact_field_notes")
            .map_err(|e| StorageError::Migration(format!("Read field_notes: {}", e)))?;

        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query field_notes: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect field_notes: {}", e)))?;

        for (contact_id, field_id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                StorageError::Migration(format!(
                    "Rekey field_note {}:{}: {}",
                    contact_id, field_id, e
                ))
            })?;
            self.conn.execute(
                "UPDATE contact_field_notes SET note_encrypted = ?1 WHERE contact_id = ?2 AND field_id = ?3",
                params![new_enc, contact_id, field_id],
            ).map_err(|e| StorageError::Migration(format!("Update field_note {}:{}: {}", contact_id, field_id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt tags: name_encrypted (owner-private tag vocabulary, ADR-051)
    pub(super) fn rekey_tags(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
        table: &str,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id, name_encrypted FROM {table}"))
            .map_err(|e| StorageError::Migration(format!("Read tags: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query tags: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect tags: {}", e)))?;

        for (id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Rekey tag {}: {}", id, e)))?;
            self.conn
                .execute(
                    &format!("UPDATE {table} SET name_encrypted = ?1 WHERE id = ?2"),
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update tag {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypts one nullable encrypted column row by row. `table` and
    /// `column` are constants from this module, never input.
    pub(super) fn rekey_nullable_column(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
        table: &str,
        column: &str,
    ) -> Result<(), StorageError> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare(&format!(
                "SELECT rowid, {column} FROM {table} WHERE {column} IS NOT NULL"
            ))
            .and_then(|mut stmt| {
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| StorageError::Migration(format!("Read {table}.{column}: {e}")))?;
        for (rowid, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Rekey {table}.{column}: {e}")))?;
            self.conn
                .execute(
                    &format!("UPDATE {table} SET {column} = ?1 WHERE rowid = ?2"),
                    params![new_enc, rowid],
                )
                .map_err(|e| StorageError::Migration(format!("Update {table}.{column}: {e}")))?;
        }
        Ok(())
    }

    /// Re-encrypt imported contacts' vCard UIDs and recompute their lookup
    /// hashes under the new key (vauchi/private#579).
    pub(super) fn rekey_contact_vcard_uids(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        use crate::storage::lookup::{CONTACT_UID_LOOKUP_DOMAIN, lookup_hmac};
        let rows: Vec<(String, Vec<u8>)> = self
            .conn
            .prepare("SELECT id, original_uid_encrypted FROM contacts WHERE original_uid_encrypted IS NOT NULL")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|e| StorageError::Migration(format!("Read vCard UIDs: {e}")))?;
        for (id, enc) in &rows {
            let uid = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt vCard UID {id}: {e}")))?;
            let new_enc = encrypt(new_key, &uid)
                .map_err(|e| StorageError::Migration(format!("Encrypt vCard UID {id}: {e}")))?;
            self.conn
                .execute(
                    "UPDATE contacts SET original_uid_encrypted = ?1, original_uid_hmac = ?2 WHERE id = ?3",
                    params![new_enc, lookup_hmac(new_key, CONTACT_UID_LOOKUP_DOMAIN, &uid), id],
                )
                .map_err(|e| StorageError::Migration(format!("Update vCard UID {id}: {e}")))?;
        }
        Ok(())
    }

    /// Re-encrypt per-contact exchange locations (nullable column, ADR-051)
    pub(super) fn rekey_exchange_location(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, exchange_location_encrypted FROM contacts WHERE exchange_location_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read exchange locations: {}", e)))?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query exchange locations: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect exchange locations: {}", e)))?;
        for (id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                StorageError::Migration(format!("Rekey exchange location {}: {}", id, e))
            })?;
            self.conn
                .execute(
                    "UPDATE contacts SET exchange_location_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| {
                    StorageError::Migration(format!("Update exchange location {}: {}", id, e))
                })?;
        }
        Ok(())
    }

    /// Re-encrypt places: data_encrypted (named-place vocabulary, ADR-051)
    pub(super) fn rekey_places(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
        table: &str,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id, data_encrypted FROM {table}"))
            .map_err(|e| StorageError::Migration(format!("Read places: {}", e)))?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query places: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect places: {}", e)))?;
        for (id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Rekey place {}: {}", id, e)))?;
            self.conn
                .execute(
                    &format!("UPDATE {table} SET data_encrypted = ?1 WHERE id = ?2"),
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update place {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contacts: cek_encrypted, visibility_rules_encrypted (nullable)
    pub(super) fn rekey_contacts_crypto(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT id, cek_encrypted, visibility_rules_encrypted FROM contacts WHERE cek_encrypted IS NOT NULL OR visibility_rules_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact crypto: {}", e)))?;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact crypto: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact crypto: {}", e)))?;

        for (id, cek_enc, vis_enc) in &rows {
            if let Some(enc) = cek_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt cek: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt cek: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE contacts SET cek_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update cek: {}", e)))?;
            }
            if let Some(enc) = vis_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt visibility_rules: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt visibility_rules: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE contacts SET visibility_rules_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update visibility_rules: {}", e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt decoy_contacts: card_encrypted (multi-row)
    pub(super) fn rekey_decoy_contacts(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, card_encrypted FROM decoy_contacts")
            .map_err(|e| StorageError::Migration(format!("Read decoys: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query decoys: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect decoys: {}", e)))?;

        for (id, enc) in &rows {
            let plain = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt decoy: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt decoy: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE decoy_contacts SET card_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update decoy: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contacts: nickname_encrypted, custom_avatar_encrypted (nullable, V43)
    pub(super) fn rekey_contact_display(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, nickname_encrypted, custom_avatar_encrypted FROM contacts WHERE nickname_encrypted IS NOT NULL OR custom_avatar_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact_display: {}", e)))?;

        type ContactDisplay = (String, Option<Vec<u8>>, Option<Vec<u8>>);
        let rows: Vec<ContactDisplay> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact_display: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact_display: {}", e)))?;

        for (id, nick_enc, avatar_enc) in &rows {
            let nick_new = if let Some(enc) = nick_enc {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt nickname {}: {}", id, e))
                })?;
                Some(encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt nickname {}: {}", id, e))
                })?)
            } else {
                None
            };

            let avatar_new = if let Some(enc) = avatar_enc {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt custom_avatar {}: {}", id, e))
                })?;
                Some(encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt custom_avatar {}: {}", id, e))
                })?)
            } else {
                None
            };

            self.conn.execute(
                "UPDATE contacts SET nickname_encrypted = ?1, custom_avatar_encrypted = ?2 WHERE id = ?3",
                params![nick_new, avatar_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact_display {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contact_shared_avatars: avatar_encrypted (V43)
    pub(super) fn rekey_shared_avatars(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, avatar_hash, avatar_encrypted FROM contact_shared_avatars")
            .map_err(|e| StorageError::Migration(format!("Read shared_avatars: {}", e)))?;

        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query shared_avatars: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect shared_avatars: {}", e)))?;

        for (contact_id, avatar_hash, enc) in &rows {
            let plain = decrypt(old_key, enc).map_err(|e| {
                StorageError::Migration(format!(
                    "Decrypt shared_avatar {}/{}: {}",
                    contact_id, avatar_hash, e
                ))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!(
                    "Encrypt shared_avatar {}/{}: {}",
                    contact_id, avatar_hash, e
                ))
            })?;
            self.conn.execute(
                "UPDATE contact_shared_avatars SET avatar_encrypted = ?1 WHERE contact_id = ?2 AND avatar_hash = ?3",
                params![new_enc, contact_id, avatar_hash],
            ).map_err(|e| StorageError::Migration(format!("Update shared_avatar {}/{}: {}", contact_id, avatar_hash, e)))?;
        }
        Ok(())
    }
}

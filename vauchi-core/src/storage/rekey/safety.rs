// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rekey of safety, recovery, deletion and UX state.

use super::*;

impl Storage {
    pub(super) fn rekey_safety_alert_facts(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, nonce, signed_payload_encrypted FROM safety_alert_facts")
            .map_err(|e| StorageError::Migration(format!("Read safety alert facts: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(|e| StorageError::Migration(format!("Query safety alert facts: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect safety alert facts: {}", e)))?;
        drop(stmt);

        for (contact_id, nonce, encrypted) in rows {
            let plain = decrypt(old_key, &encrypted).map_err(|e| {
                StorageError::Migration(format!("Decrypt safety alert fact: {}", e))
            })?;
            let reencrypted = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt safety alert fact: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE safety_alert_facts SET signed_payload_encrypted = ?1
                         WHERE contact_id = ?2 AND nonce = ?3",
                    params![reencrypted, contact_id, nonce],
                )
                .map_err(|e| StorageError::Migration(format!("Update safety alert fact: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt recovery_responses: response_encrypted
    pub(super) fn rekey_recovery_responses(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT claim_id, response_encrypted FROM recovery_responses WHERE response_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read recovery: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query recovery: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect recovery: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt recovery {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt recovery {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE recovery_responses SET response_encrypted = ?1 WHERE claim_id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update recovery {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt deletion_state: state_json_encrypted
    pub(super) fn rekey_deletion_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT state_json_encrypted FROM deletion_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt deletion_state: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt deletion_state: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE deletion_state SET state_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update deletion_state: {}", e)))?;
        }
        Ok(())
    }

    pub(super) fn rekey_ux_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result = self.conn.query_row(
            "SELECT id, aha_tracker_json_encrypted, demo_contact_json_encrypted, onboarding_progress_encrypted, backup_reminder_encrypted, settings_flags_encrypted, relay_url_encrypted, own_card_repropagate_encrypted, exchange_defaults_encrypted, relay_anchor_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let id: i64 = row.get(0)?;
                let aha: Option<Vec<u8>> = row.get(1)?;
                let demo: Option<Vec<u8>> = row.get(2)?;
                let onboarding: Option<Vec<u8>> = row.get(3)?;
                let backup_reminder: Option<Vec<u8>> = row.get(4)?;
                let settings_flags: Option<Vec<u8>> = row.get(5)?;
                let relay_url: Option<Vec<u8>> = row.get(6)?;
                let own_card_repropagate: Option<Vec<u8>> = row.get(7)?;
                let exchange_defaults: Option<Vec<u8>> = row.get(8)?;
                let relay_anchor: Option<Vec<u8>> = row.get(9)?;
                Ok((
                    id,
                    aha,
                    demo,
                    onboarding,
                    backup_reminder,
                    settings_flags,
                    relay_url,
                    own_card_repropagate,
                    exchange_defaults,
                    relay_anchor,
                ))
            },
        );

        if let Ok((
            id,
            aha_enc,
            demo_enc,
            onboarding_enc,
            backup_reminder_enc,
            settings_flags_enc,
            relay_url_enc,
            own_card_repropagate_enc,
            exchange_defaults_enc,
            relay_anchor_enc,
        )) = result
        {
            if let Some(enc) = aha_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt aha_tracker: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt aha_tracker: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET aha_tracker_json_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update aha_tracker: {}", e)))?;
            }
            if let Some(enc) = demo_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt demo_contact: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt demo_contact: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET demo_contact_json_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update demo_contact: {}", e)))?;
            }
            if let Some(enc) = onboarding_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt onboarding: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt onboarding: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET onboarding_progress_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update onboarding: {}", e)))?;
            }
            if let Some(enc) = backup_reminder_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt backup_reminder: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt backup_reminder: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET backup_reminder_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update backup_reminder: {}", e))
                    })?;
            }
            if let Some(enc) = settings_flags_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt settings_flags: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt settings_flags: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET settings_flags_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update settings_flags: {}", e))
                    })?;
            }
            if let Some(enc) = relay_url_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt relay_url: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt relay_url: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET relay_url_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update relay_url: {}", e)))?;
            }
            if let Some(enc) = relay_anchor_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt relay_anchor: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt relay_anchor: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET relay_anchor_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update relay_anchor: {}", e)))?;
            }
            if let Some(enc) = own_card_repropagate_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt own_card_repropagate: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt own_card_repropagate: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET own_card_repropagate_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update own_card_repropagate: {}", e))
                    })?;
            }
            if let Some(enc) = exchange_defaults_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt exchange_defaults: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt exchange_defaults: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET exchange_defaults_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update exchange_defaults: {}", e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt audit_log: details_encrypted
    pub(super) fn rekey_audit_log(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, details_encrypted FROM audit_log WHERE details_encrypted IS NOT NULL",
            )
            .map_err(|e| StorageError::Migration(format!("Read audit_log for rekey: {}", e)))?;
        let rows: Vec<(i64, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query audit_log: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect audit_log: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt audit_log {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt audit_log {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE audit_log SET details_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update audit_log {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt emergency_config: trusted_contact_ids_encrypted, message_encrypted
    pub(super) fn rekey_emergency_config(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        #[allow(clippy::type_complexity)]
        let result: Result<(Option<Vec<u8>>, Option<Vec<u8>>), _> = self.conn.query_row(
            "SELECT trusted_contact_ids_encrypted, message_encrypted FROM emergency_config WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((ids_enc, msg_enc)) = result {
            if let Some(enc) = ids_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt emergency_ids: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt emergency_ids: {}", e))
                })?;
                self.conn.execute(
                        "UPDATE emergency_config SET trusted_contact_ids_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    ).map_err(|e| StorageError::Migration(format!("Update emergency_ids: {}", e)))?;
            }
            if let Some(enc) = msg_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt emergency_msg: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt emergency_msg: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE emergency_config SET message_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update emergency_msg: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt recovery_settings: settings_encrypted (singleton)
    pub(super) fn rekey_recovery_settings(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT settings_encrypted FROM recovery_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt recovery_settings: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt recovery_settings: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE recovery_settings SET settings_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update recovery_settings: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt recovery_progress: progress_encrypted (singleton, V44)
    pub(super) fn rekey_recovery_progress(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT progress_encrypted FROM recovery_progress WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt recovery_progress: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt recovery_progress: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE recovery_progress SET progress_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update recovery_progress: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt exchange_states: encrypted_blob (multi-row, crash recovery)
    pub(super) fn rekey_exchange_states(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT exchange_id, encrypted_blob FROM exchange_states")
            .map_err(|e| StorageError::Migration(format!("Read exchange_states: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query exchange_states: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect exchange_states: {}", e)))?;

        for (id, enc) in &rows {
            let plain = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt exchange_state: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt exchange_state: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE exchange_states SET encrypted_blob = ?1 WHERE exchange_id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update exchange_state: {}", e)))?;
        }
        Ok(())
    }
}

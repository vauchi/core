// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Field validation storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::social::ProfileValidation;

/// SQL columns selected for validation queries (with encrypted fields).
const VALIDATION_SELECT: &str = "field_id, field_value_encrypted, field_value, validator_id, validated_at, signature_encrypted, signature";

/// Intermediate row before field decryption.
struct ValidationRow {
    field_id: String,
    field_value_encrypted: Option<Vec<u8>>,
    field_value_plaintext: String,
    validator_id: String,
    validated_at: i64,
    signature_encrypted: Option<Vec<u8>>,
    signature_plaintext: Vec<u8>,
}

fn row_to_validation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ValidationRow> {
    Ok(ValidationRow {
        field_id: row.get(0)?,
        field_value_encrypted: row.get(1)?,
        field_value_plaintext: row.get(2)?,
        validator_id: row.get(3)?,
        validated_at: row.get(4)?,
        signature_encrypted: row.get(5)?,
        signature_plaintext: row.get(6)?,
    })
}

impl Storage {
    /// Decrypts a ValidationRow and converts to ProfileValidation.
    fn decrypt_validation_row(
        &self,
        row: ValidationRow,
    ) -> Result<ProfileValidation, StorageError> {
        // Decrypt field_value
        let field_value = if let Some(enc) = row.field_value_encrypted {
            if !enc.is_empty() {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?
            } else {
                row.field_value_plaintext
            }
        } else {
            row.field_value_plaintext
        };

        // Decrypt signature
        let signature_bytes = if let Some(enc) = row.signature_encrypted {
            if !enc.is_empty() {
                crate::crypto::decrypt(&self.encryption_key, &enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?
            } else {
                row.signature_plaintext
            }
        } else {
            row.signature_plaintext
        };

        let signature: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| StorageError::InvalidData("invalid signature length".into()))?;

        Ok(ProfileValidation::from_stored(
            &row.field_id,
            &field_value,
            &row.validator_id,
            row.validated_at as u64,
            signature,
        ))
    }

    /// Saves a field validation to storage (encrypted).
    ///
    /// The validation is stored with a unique constraint on
    /// (contact_id, field_id, validator_id) to prevent duplicate validations.
    pub fn save_validation(&self, validation: &ProfileValidation) -> Result<(), StorageError> {
        let contact_id = validation
            .contact_id()
            .ok_or_else(|| StorageError::InvalidData("validation missing contact_id".into()))?;

        let id = format!(
            "{}:{}:{}",
            contact_id,
            validation.field_id(),
            validation.validator_id()
        );

        let field_value_encrypted =
            crate::crypto::encrypt(&self.encryption_key, validation.field_value().as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let signature_encrypted =
            crate::crypto::encrypt(&self.encryption_key, validation.signature().as_slice())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO field_validations
             (id, contact_id, field_id, field_value, field_value_encrypted, validator_id, validated_at, signature, signature_encrypted)
             VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, X'', ?7)",
            params![
                id,
                contact_id,
                validation.field_id(),
                field_value_encrypted,
                validation.validator_id(),
                validation.validated_at() as i64,
                signature_encrypted,
            ],
        )?;

        Ok(())
    }

    /// Loads all validations for a specific field.
    ///
    /// ## Note (Tracker #112)
    ///
    /// Loaded validations are decrypted but their Ed25519 signatures are not
    /// re-verified against the validator's public key. A corrupted or tampered
    /// database could return validations with invalid signatures. Callers that
    /// display trust-sensitive information should re-verify signatures via
    /// `ProfileValidation::verify()`.
    pub fn load_validations_for_field(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<Vec<ProfileValidation>, StorageError> {
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let sql = format!(
            "SELECT {} FROM field_validations WHERE contact_id = ?1 AND field_id = ?2",
            VALIDATION_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<ValidationRow> = stmt
            .query_map(params![contact_id, full_field_id], row_to_validation_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_validation_row(r))
            .collect()
    }

    /// Loads all validations made by a specific validator (for listing my validations).
    pub fn load_validations_by_validator(
        &self,
        validator_id: &str,
    ) -> Result<Vec<ProfileValidation>, StorageError> {
        let sql = format!(
            "SELECT {} FROM field_validations WHERE validator_id = ?1 ORDER BY validated_at DESC",
            VALIDATION_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;

        let rows: Vec<ValidationRow> = stmt
            .query_map(params![validator_id], row_to_validation_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)?;

        rows.into_iter()
            .map(|r| self.decrypt_validation_row(r))
            .collect()
    }

    /// Deletes a validation (revokes my validation of a field).
    ///
    /// Returns true if a validation was deleted, false if none existed.
    pub fn delete_validation(
        &self,
        contact_id: &str,
        field_id: &str,
        validator_id: &str,
    ) -> Result<bool, StorageError> {
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let rows_affected = self.conn.execute(
            "DELETE FROM field_validations
             WHERE contact_id = ?1 AND field_id = ?2 AND validator_id = ?3",
            params![contact_id, full_field_id, validator_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Deletes all validations for a field (called when field value changes).
    pub fn delete_validations_for_field(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<usize, StorageError> {
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let rows_affected = self.conn.execute(
            "DELETE FROM field_validations WHERE contact_id = ?1 AND field_id = ?2",
            params![contact_id, full_field_id],
        )?;

        Ok(rows_affected)
    }

    /// Counts validations for a field (useful for quick status checks).
    pub fn count_validations_for_field(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<usize, StorageError> {
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM field_validations
             WHERE contact_id = ?1 AND field_id = ?2",
            params![contact_id, full_field_id],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }

    /// Checks if a specific validator has validated a field.
    pub fn has_validated(
        &self,
        contact_id: &str,
        field_id: &str,
        validator_id: &str,
    ) -> Result<bool, StorageError> {
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM field_validations
             WHERE contact_id = ?1 AND field_id = ?2 AND validator_id = ?3",
            params![contact_id, full_field_id, validator_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }
}

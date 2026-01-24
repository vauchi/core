//! Field validation storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::social::ProfileValidation;

impl Storage {
    /// Saves a field validation to storage.
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

        self.conn.execute(
            "INSERT OR REPLACE INTO field_validations
             (id, contact_id, field_id, field_value, validator_id, validated_at, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                contact_id,
                validation.field_id(),
                validation.field_value(),
                validation.validator_id(),
                validation.validated_at() as i64,
                validation.signature().as_slice(),
            ],
        )?;

        Ok(())
    }

    /// Loads all validations for a specific field.
    pub fn load_validations_for_field(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<Vec<ProfileValidation>, StorageError> {
        // The field_id in the validation is formatted as "contact_id:field_name"
        let full_field_id = format!("{}:{}", contact_id, field_id);

        let mut stmt = self.conn.prepare(
            "SELECT field_id, field_value, validator_id, validated_at, signature
             FROM field_validations
             WHERE contact_id = ?1 AND field_id = ?2",
        )?;

        let rows = stmt.query_map(params![contact_id, full_field_id], |row| {
            let field_id: String = row.get(0)?;
            let field_value: String = row.get(1)?;
            let validator_id: String = row.get(2)?;
            let validated_at: i64 = row.get(3)?;
            let signature_bytes: Vec<u8> = row.get(4)?;

            Ok((
                field_id,
                field_value,
                validator_id,
                validated_at,
                signature_bytes,
            ))
        })?;

        let mut validations = Vec::new();
        for row_result in rows {
            let (field_id, field_value, validator_id, validated_at, signature_bytes) = row_result?;

            let signature: [u8; 64] = signature_bytes
                .try_into()
                .map_err(|_| StorageError::InvalidData("invalid signature length".into()))?;

            // Use the internal constructor that accepts all fields
            let validation = ProfileValidation::from_stored(
                &field_id,
                &field_value,
                &validator_id,
                validated_at as u64,
                signature,
            );

            validations.push(validation);
        }

        Ok(validations)
    }

    /// Loads all validations made by a specific validator (for listing my validations).
    pub fn load_validations_by_validator(
        &self,
        validator_id: &str,
    ) -> Result<Vec<ProfileValidation>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT field_id, field_value, validator_id, validated_at, signature
             FROM field_validations
             WHERE validator_id = ?1
             ORDER BY validated_at DESC",
        )?;

        let rows = stmt.query_map(params![validator_id], |row| {
            let field_id: String = row.get(0)?;
            let field_value: String = row.get(1)?;
            let validator_id: String = row.get(2)?;
            let validated_at: i64 = row.get(3)?;
            let signature_bytes: Vec<u8> = row.get(4)?;

            Ok((
                field_id,
                field_value,
                validator_id,
                validated_at,
                signature_bytes,
            ))
        })?;

        let mut validations = Vec::new();
        for row_result in rows {
            let (field_id, field_value, validator_id, validated_at, signature_bytes) = row_result?;

            let signature: [u8; 64] = signature_bytes
                .try_into()
                .map_err(|_| StorageError::InvalidData("invalid signature length".into()))?;

            let validation = ProfileValidation::from_stored(
                &field_id,
                &field_value,
                &validator_id,
                validated_at as u64,
                signature,
            );

            validations.push(validation);
        }

        Ok(validations)
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

//! Delivery record storage operations.

use rusqlite::params;

use super::error::{DeliveryRecord, DeliveryStatus};
use super::{Storage, StorageError};

impl Storage {
    // === Delivery Records Operations ===

    /// Creates a new delivery record.
    pub fn create_delivery_record(&self, record: &DeliveryRecord) -> Result<(), StorageError> {
        let (status_str, status_reason) = status_to_db(&record.status);

        self.conn.execute(
            "INSERT INTO delivery_records
             (message_id, recipient_id, status, status_reason, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.message_id,
                record.recipient_id,
                status_str,
                status_reason,
                record.created_at as i64,
                record.updated_at as i64,
                record.expires_at.map(|t| t as i64),
            ],
        )?;

        Ok(())
    }

    /// Gets a delivery record by message ID.
    pub fn get_delivery_record(
        &self,
        message_id: &str,
    ) -> Result<Option<DeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, status, status_reason, created_at, updated_at, expires_at
             FROM delivery_records WHERE message_id = ?1",
        )?;

        let mut rows = stmt.query(params![message_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_delivery_record(row)?)),
            None => Ok(None),
        }
    }

    /// Gets all delivery records for a recipient.
    pub fn get_delivery_records_for_recipient(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<DeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, status, status_reason, created_at, updated_at, expires_at
             FROM delivery_records WHERE recipient_id = ?1 ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map(params![recipient_id], |row| row_to_delivery_record(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Gets all delivery records with a specific status.
    pub fn get_delivery_records_by_status(
        &self,
        status: &DeliveryStatus,
    ) -> Result<Vec<DeliveryRecord>, StorageError> {
        let (status_str, _) = status_to_db(status);

        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, status, status_reason, created_at, updated_at, expires_at
             FROM delivery_records WHERE status = ?1 ORDER BY created_at",
        )?;

        let rows = stmt.query_map(params![status_str], |row| row_to_delivery_record(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Updates the status of a delivery record.
    pub fn update_delivery_status(
        &self,
        message_id: &str,
        status: &DeliveryStatus,
        updated_at: u64,
    ) -> Result<bool, StorageError> {
        let (status_str, status_reason) = status_to_db(status);

        let rows_affected = self.conn.execute(
            "UPDATE delivery_records SET status = ?1, status_reason = ?2, updated_at = ?3
             WHERE message_id = ?4",
            params![status_str, status_reason, updated_at as i64, message_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Deletes a delivery record.
    pub fn delete_delivery_record(&self, message_id: &str) -> Result<bool, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM delivery_records WHERE message_id = ?1",
            params![message_id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Gets pending (non-terminal) delivery records that haven't been fully delivered.
    pub fn get_pending_deliveries(&self) -> Result<Vec<DeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, recipient_id, status, status_reason, created_at, updated_at, expires_at
             FROM delivery_records
             WHERE status NOT IN ('delivered', 'expired', 'failed')
             ORDER BY created_at",
        )?;

        let rows = stmt.query_map([], |row| row_to_delivery_record(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Marks expired delivery records as expired.
    pub fn expire_old_deliveries(&self, now: u64) -> Result<usize, StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE delivery_records SET status = 'expired', updated_at = ?1
             WHERE expires_at IS NOT NULL AND expires_at < ?1
             AND status NOT IN ('delivered', 'expired', 'failed')",
            params![now as i64],
        )?;
        Ok(rows_affected)
    }

    /// Counts delivery records by status.
    pub fn count_deliveries_by_status(
        &self,
        status: &DeliveryStatus,
    ) -> Result<usize, StorageError> {
        let (status_str, _) = status_to_db(status);

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM delivery_records WHERE status = ?1",
            params![status_str],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

/// Converts DeliveryStatus to database representation.
fn status_to_db(status: &DeliveryStatus) -> (&'static str, Option<String>) {
    match status {
        DeliveryStatus::Queued => ("queued", None),
        DeliveryStatus::Sent => ("sent", None),
        DeliveryStatus::Stored => ("stored", None),
        DeliveryStatus::Delivered => ("delivered", None),
        DeliveryStatus::Expired => ("expired", None),
        DeliveryStatus::Failed { reason } => ("failed", Some(reason.clone())),
    }
}

/// Converts database row to DeliveryRecord.
fn row_to_delivery_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeliveryRecord> {
    let status_str: String = row.get(2)?;
    let status_reason: Option<String> = row.get(3)?;

    let status = match status_str.as_str() {
        "queued" => DeliveryStatus::Queued,
        "sent" => DeliveryStatus::Sent,
        "stored" => DeliveryStatus::Stored,
        "delivered" => DeliveryStatus::Delivered,
        "expired" => DeliveryStatus::Expired,
        "failed" => DeliveryStatus::Failed {
            reason: status_reason.unwrap_or_default(),
        },
        _ => DeliveryStatus::Queued,
    };

    Ok(DeliveryRecord {
        message_id: row.get(0)?,
        recipient_id: row.get(1)?,
        status,
        created_at: row.get::<_, i64>(4)? as u64,
        updated_at: row.get::<_, i64>(5)? as u64,
        expires_at: row.get::<_, Option<i64>>(6)?.map(|t| t as u64),
    })
}

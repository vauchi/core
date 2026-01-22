//! Per-device delivery tracking storage operations.

use rusqlite::params;

use super::error::{DeliverySummary, DeviceDeliveryRecord, DeviceDeliveryStatus};
use super::{Storage, StorageError};

impl Storage {
    // === Device Delivery Operations ===

    /// Creates a new device delivery record.
    pub fn create_device_delivery(&self, record: &DeviceDeliveryRecord) -> Result<(), StorageError> {
        let status_str = status_to_str(&record.status);

        self.conn.execute(
            "INSERT INTO device_deliveries
             (message_id, device_id, recipient_id, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.message_id,
                record.device_id,
                record.recipient_id,
                status_str,
                record.updated_at as i64,
            ],
        )?;

        Ok(())
    }

    /// Gets a device delivery record.
    pub fn get_device_delivery(
        &self,
        message_id: &str,
        device_id: &str,
    ) -> Result<Option<DeviceDeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, device_id, recipient_id, status, updated_at
             FROM device_deliveries WHERE message_id = ?1 AND device_id = ?2",
        )?;

        let mut rows = stmt.query(params![message_id, device_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_device_delivery(row)?)),
            None => Ok(None),
        }
    }

    /// Gets all device delivery records for a message.
    pub fn get_device_deliveries_for_message(
        &self,
        message_id: &str,
    ) -> Result<Vec<DeviceDeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, device_id, recipient_id, status, updated_at
             FROM device_deliveries WHERE message_id = ?1 ORDER BY device_id",
        )?;

        let rows = stmt.query_map(params![message_id], |row| row_to_device_delivery(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Updates the status of a device delivery.
    pub fn update_device_delivery_status(
        &self,
        message_id: &str,
        device_id: &str,
        status: DeviceDeliveryStatus,
        updated_at: u64,
    ) -> Result<bool, StorageError> {
        let status_str = status_to_str(&status);

        let rows_affected = self.conn.execute(
            "UPDATE device_deliveries SET status = ?1, updated_at = ?2
             WHERE message_id = ?3 AND device_id = ?4",
            params![status_str, updated_at as i64, message_id, device_id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Gets delivery summary for a message (X of Y devices delivered).
    pub fn get_delivery_summary(&self, message_id: &str) -> Result<DeliverySummary, StorageError> {
        let records = self.get_device_deliveries_for_message(message_id)?;

        let total_devices = records.len();
        let delivered_devices = records
            .iter()
            .filter(|r| r.status == DeviceDeliveryStatus::Delivered)
            .count();
        let pending_devices = records
            .iter()
            .filter(|r| {
                r.status == DeviceDeliveryStatus::Pending
                    || r.status == DeviceDeliveryStatus::Stored
            })
            .count();
        let failed_devices = records
            .iter()
            .filter(|r| r.status == DeviceDeliveryStatus::Failed)
            .count();

        Ok(DeliverySummary {
            message_id: message_id.to_string(),
            total_devices,
            delivered_devices,
            pending_devices,
            failed_devices,
        })
    }

    /// Deletes all device delivery records for a message.
    pub fn delete_device_deliveries_for_message(
        &self,
        message_id: &str,
    ) -> Result<usize, StorageError> {
        let rows_affected = self.conn.execute(
            "DELETE FROM device_deliveries WHERE message_id = ?1",
            params![message_id],
        )?;
        Ok(rows_affected)
    }

    /// Gets all pending device deliveries (not yet delivered).
    pub fn get_pending_device_deliveries(&self) -> Result<Vec<DeviceDeliveryRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id, device_id, recipient_id, status, updated_at
             FROM device_deliveries WHERE status IN ('pending', 'stored')
             ORDER BY updated_at",
        )?;

        let rows = stmt.query_map([], |row| row_to_device_delivery(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    /// Counts device deliveries by status.
    pub fn count_device_deliveries_by_status(
        &self,
        status: DeviceDeliveryStatus,
    ) -> Result<usize, StorageError> {
        let status_str = status_to_str(&status);

        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM device_deliveries WHERE status = ?1",
            params![status_str],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

/// Converts DeviceDeliveryStatus to database string.
fn status_to_str(status: &DeviceDeliveryStatus) -> &'static str {
    match status {
        DeviceDeliveryStatus::Pending => "pending",
        DeviceDeliveryStatus::Stored => "stored",
        DeviceDeliveryStatus::Delivered => "delivered",
        DeviceDeliveryStatus::Failed => "failed",
    }
}

/// Converts database row to DeviceDeliveryRecord.
fn row_to_device_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceDeliveryRecord> {
    let status_str: String = row.get(3)?;

    let status = match status_str.as_str() {
        "pending" => DeviceDeliveryStatus::Pending,
        "stored" => DeviceDeliveryStatus::Stored,
        "delivered" => DeviceDeliveryStatus::Delivered,
        "failed" => DeviceDeliveryStatus::Failed,
        _ => DeviceDeliveryStatus::Pending,
    };

    Ok(DeviceDeliveryRecord {
        message_id: row.get(0)?,
        device_id: row.get(1)?,
        recipient_id: row.get(2)?,
        status,
        updated_at: row.get::<_, i64>(4)? as u64,
    })
}

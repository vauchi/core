// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `delivery` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_delivery(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::Sync => {
                let vauchi = engine.vauchi_mut();
                if !vauchi.has_ohttp_key() {
                    vauchi.connect().map_err(|e| MobileError::Other {
                        detail: format!("Connect: {e}"),
                    })?;
                }
                let outcome = vauchi.sync().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let result = crate::types::MobileSyncResult::try_from(outcome)?;
                Ok(DomainCommandResult::SyncResult { result })
            }
            DomainCommand::PendingUpdateCount => {
                let storage = engine.vauchi().storage();
                let contacts =
                    storage
                        .contacts()
                        .list_contacts()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                let mut total: u32 = 0;
                for contact in contacts {
                    let pending = storage
                        .pending()
                        .get_pending_updates(contact.id())
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                    total += pending.len() as u32;
                }
                Ok(DomainCommandResult::Count { value: total })
            }
            DomainCommand::GetDeliveryRecord { message_id } => {
                let record = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .get_delivery_record(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecordOpt {
                    record: record
                        .as_ref()
                        .map(crate::types::MobileDeliveryRecord::from),
                })
            }
            DomainCommand::GetAllDeliveryRecords => {
                let records = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .get_all_delivery_records()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetDeliveryRecordsForContact { recipient_id } => {
                let records = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .get_delivery_records_for_recipient(&recipient_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::CountFailedDeliveries => {
                let count = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .count_deliveries_by_status(&vauchi_core::storage::DeliveryStatus::Failed {
                        reason: String::new(),
                    })
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetFailedDeliveryRecords => {
                let records = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .get_delivery_records_by_status(&vauchi_core::storage::DeliveryStatus::Failed {
                        reason: String::new(),
                    })
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::ManualRetry { message_id } => {
                let storage = engine.vauchi().storage();
                let entry = storage
                    .retries()
                    .get_retry_entry(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                if entry.is_none() {
                    return Ok(DomainCommandResult::Bool { value: false });
                }
                let now = engine.vauchi().clock().unix_seconds();
                storage
                    .retries()
                    .update_retry_next_time(&message_id, now)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DeliveryStatus);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::GetPendingDeliveries => {
                let records = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .get_pending_deliveries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetDeliveryCountByStatus { status } => {
                use vauchi_core::storage::DeliveryStatus;
                let core_status = match status {
                    crate::types::MobileDeliveryStatus::Queued => DeliveryStatus::Queued,
                    crate::types::MobileDeliveryStatus::Sent => DeliveryStatus::Sent,
                    crate::types::MobileDeliveryStatus::Stored => DeliveryStatus::Stored,
                    crate::types::MobileDeliveryStatus::Delivered => DeliveryStatus::Delivered,
                    crate::types::MobileDeliveryStatus::Expired => DeliveryStatus::Expired,
                    crate::types::MobileDeliveryStatus::Failed => DeliveryStatus::Failed {
                        reason: String::new(),
                    },
                };
                let count = engine
                    .vauchi()
                    .storage()
                    .deliveries()
                    .count_deliveries_by_status(&core_status)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetDueRetries => {
                let now = engine.vauchi().clock().unix_seconds();
                let entries = engine
                    .vauchi()
                    .storage()
                    .retries()
                    .get_due_retries(now)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::RetryEntries {
                    entries: entries
                        .iter()
                        .map(crate::types::MobileRetryEntry::from)
                        .collect(),
                })
            }
            DomainCommand::GetRetriesForContact { contact_id } => {
                let entries = engine
                    .vauchi()
                    .storage()
                    .retries()
                    .get_retry_entries_for_recipient(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::RetryEntries {
                    entries: entries
                        .iter()
                        .map(crate::types::MobileRetryEntry::from)
                        .collect(),
                })
            }
            DomainCommand::GetRetryCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .retries()
                    .count_retry_entries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::DeleteRetry { message_id } => {
                let deleted = engine
                    .vauchi()
                    .storage()
                    .retries()
                    .delete_retry_entry(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                if deleted {
                    engine.invalidate_screen(&AppScreen::DeliveryStatus);
                }
                Ok(DomainCommandResult::Bool { value: deleted })
            }
            DomainCommand::CalculateRetryBackoff { attempt } => {
                let queue = vauchi_core::storage::RetryQueue::new();
                Ok(DomainCommandResult::BackoffSeconds {
                    seconds: queue.backoff_seconds(attempt),
                })
            }
            DomainCommand::GetTotalPendingCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .pending()
                    .count_all_pending_updates()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::IsOfflineQueueFull => {
                let queue = vauchi_core::storage::OfflineQueue::new();
                let value = queue.is_full(engine.vauchi().storage()).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::GetOfflineQueueCapacity => {
                let queue = vauchi_core::storage::OfflineQueue::new();
                let remaining = queue
                    .remaining_capacity(engine.vauchi().storage())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: remaining as u32,
                })
            }
            DomainCommand::ClearPendingUpdatesForContact { contact_id } => {
                let count = engine
                    .vauchi()
                    .storage()
                    .pending()
                    .delete_pending_updates_for_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DeliveryStatus);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetDeliverySummary { message_id } => {
                let summary = engine
                    .vauchi()
                    .storage()
                    .device_deliveries()
                    .get_delivery_summary(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliverySummary {
                    summary: crate::types::MobileDeliverySummary::from(&summary),
                })
            }
            DomainCommand::GetDeviceDeliveries { message_id } => {
                let records = engine
                    .vauchi()
                    .storage()
                    .device_deliveries()
                    .get_device_deliveries_for_message(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeviceDeliveries {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeviceDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetPendingDeviceDeliveries => {
                let records = engine
                    .vauchi()
                    .storage()
                    .device_deliveries()
                    .get_pending_device_deliveries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeviceDeliveries {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeviceDeliveryRecord::from)
                        .collect(),
                })
            }

            // ── Identity reads + Onboarding helpers (B7 batch 9) ──
            other => unreachable!("non-delivery command {other:?} routed to delivery dispatcher"),
        }
    }
}

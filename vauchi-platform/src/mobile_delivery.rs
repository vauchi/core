// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync, failed-delivery reads, backup operations.
//!
//! After slice 32h Phase 1 (2026-05-18), 22 storage-only delegation
//! methods retired (DC wired in PAE, zero binding consumers). The 4
//! delivery-flag accessors (`is/set_delivery_receipts_enabled`,
//! `is/set_suppress_presence_enabled`) also retired in the 2026-05-18
//! Phase 2a flag-vestigial cleanup — they were dead getter/setter
//! pairs over `Mutex<bool>` fields that VauchiPlatform never read
//! internally; the real flag state persists on `PlatformAppEngine`'s
//! side (`load_sync_flags_engine` / `save_sync_flags_engine`).
//!
//! 6 methods remain pending Phase 2:
//!
//! - **Phase 2a (G4b)**: 3 trapped methods that have lib.rs internal
//!   tests / `tests/it/` / `benches/` callers — `export_backup`,
//!   `import_backup`, `get_failed_delivery_records`.
//! - **Phase 2b (sync orchestration design)**: 3 sync-state methods
//!   that need engine-resident sync state — `sync`, `get_sync_status`,
//!   `sync_async`. Documented as a separate batch at
//!   `domain_command.rs:307-313`.

use std::sync::Arc;

use vauchi_core::{ContactCard, Identity, IdentityBackup};

use super::error::{MobileError, lock_or};
use super::types::{MobileDeliveryRecord, MobileSyncResult, MobileSyncStatus};
use super::{IdentityData, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === Sync Operations ===

    /// Sync with relay server via OHTTP-encrypted HTTP.
    ///
    /// Creates a temporary `Vauchi` instance, connects, syncs, and maps the
    /// outcome to `MobileSyncResult`. All synchronous — no tokio runtime needed.
    pub fn sync(&self) -> Result<MobileSyncResult, MobileError> {
        use vauchi_core::api::VauchiSyncOutcome;

        *lock_or(&self.sync_status)? = MobileSyncStatus::Syncing;

        let result = (|| -> Result<MobileSyncResult, MobileError> {
            // open_vauchi_for_relay() loads identity via self.get_identity()
            // (avoiding the silent `.ok()` swallow in Vauchi::init that
            // returns IdentityNotInitialized when from_storage_bytes parsing
            // fails) and pre-resolves the OHTTP gateway key.
            let mut vauchi = self.open_vauchi_for_relay()?;
            vauchi.connect().map_err(|e| MobileError::Other {
                detail: format!("Connect: {e}"),
            })?;

            let outcome = vauchi.sync().map_err(|e| MobileError::Other {
                detail: format!("{e}"),
            })?;

            vauchi.disconnect();

            match outcome {
                VauchiSyncOutcome::Ok {
                    received,
                    sent,
                    acknowledged: _,
                    errors: _,
                    version_policy: _,
                } => Ok(MobileSyncResult {
                    contacts_added: 0,
                    cards_updated: received as u32,
                    updates_sent: sent as u32,
                    total: (received + sent) as u32,
                    has_changes: received > 0 || sent > 0,
                    updated_contact_names: vec![],
                }),
                VauchiSyncOutcome::TooSoon => Ok(MobileSyncResult {
                    contacts_added: 0,
                    cards_updated: 0,
                    updates_sent: 0,
                    total: 0,
                    has_changes: false,
                    updated_contact_names: vec![],
                }),
                VauchiSyncOutcome::NotConnected => Err(MobileError::Other {
                    detail: "Not connected".into(),
                }),
                VauchiSyncOutcome::NoIdentity => Err(MobileError::Other {
                    detail: "No identity".into(),
                }),
            }
        })();

        match &result {
            Ok(_) => {
                let _ = lock_or(&self.sync_status).map(|mut g| *g = MobileSyncStatus::Idle);
            }
            Err(_) => {
                let _ = lock_or(&self.sync_status).map(|mut g| *g = MobileSyncStatus::Error);
            }
        }

        result
    }

    /// Get sync status.
    pub fn get_sync_status(&self) -> MobileSyncStatus {
        let Ok(guard) = self.sync_status.lock() else {
            return MobileSyncStatus::Error;
        };
        *guard
    }

    // === Failed Delivery Reads (kept — tests/it caller) ===

    /// Get all failed delivery records.
    ///
    /// Frontends should call this instead of fetching `get_all_delivery_records()`
    /// and filtering by `status == Failed` themselves — see ADR-021/043
    /// (the Humble UI). The partition decision lives in core so iOS, Android,
    /// and any future frontend render the same list without divergence.
    pub fn get_failed_delivery_records(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let storage = self.open_storage()?;
        let records = storage.get_delivery_records_by_status(&DeliveryStatus::Failed {
            reason: String::new(),
        })?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    // === Backup Operations ===

    /// Export encrypted backup.
    pub fn export_backup(&self, password: String) -> Result<String, MobileError> {
        let identity = self.get_identity()?;

        let backup = identity
            .export_backup(&password)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(backup.as_bytes());

        Ok(encoded)
    }

    /// Import backup.
    pub fn import_backup(&self, backup_data: String, password: String) -> Result<(), MobileError> {
        {
            let data = lock_or(&self.identity_data)?;
            if data.is_some() {
                return Err(MobileError::Other {
                    detail: "Already initialized".to_string(),
                });
            }
        }

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&backup_data)
            .map_err(|_| MobileError::InvalidInput {
                field: String::new(),
                detail: "Invalid base64".to_string(),
            })?;

        let backup = IdentityBackup::new(bytes);
        let identity = Identity::import_backup(
            &backup,
            &password,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })?;

        let internal_backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;

        let internal_backup_data = internal_backup.as_bytes().to_vec();
        let display_name = identity.display_name().to_string();

        let storage = self.open_storage()?;
        storage.save_identity(&internal_backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data: internal_backup_data,
            display_name: display_name.clone(),
        };
        *lock_or(&self.identity_data)? = Some(identity_data);

        if storage.load_own_card()?.is_none() {
            let card = ContactCard::new(&display_name);
            storage.save_own_card(&card)?;
        }

        Ok(())
    }
}

// Async sync method — runs sync on a blocking thread to prevent UI freeze.
// Feature-gated behind `async-sync` (default) which pulls in tokio.
#[cfg(feature = "async-sync")]
#[uniffi::export(async_runtime = "tokio")]
impl VauchiPlatform {
    /// Async version of sync for mobile UI threads.
    ///
    /// Delegates to the synchronous `sync()` via `spawn_blocking` so
    /// the core OHTTP HTTP calls don't block the async runtime.
    pub async fn sync_async(self: Arc<Self>) -> Result<MobileSyncResult, MobileError> {
        let platform = self.clone();
        tokio::task::spawn_blocking(move || platform.sync())
            .await
            .map_err(|e| MobileError::Other {
                detail: format!("Sync task panicked: {e}"),
            })?
    }
}

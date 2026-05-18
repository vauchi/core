// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync orchestration (the last cluster — Phase 2b).
//!
//! Slice 32h Phase 1 (2026-05-18) retired 22 storage-only delegations
//! (DC wired in PAE, zero binding consumers). Subsequent 2026-05-18
//! Phase 2a iterations cleared all remaining wrappers from this file:
//!
//! - Flag-vestigial cleanup: `is/set_delivery_receipts_enabled`,
//!   `is/set_suppress_presence_enabled` (dead getter/setter pairs over
//!   `Mutex<bool>` fields that were never read; real flag state is on
//!   PAE side via `load_sync_flags_engine`).
//! - Backup-cluster delete-pair: `export_backup`, `import_backup`
//!   (redundant — covered by `full_backup_api_tests.rs` et al.).
//! - get-failed-delivery relocate: `get_failed_delivery_records`
//!   (Humble-UI contract test migrated to PAE+dispatch in
//!   `tests/it/mobile_delivery_tests.rs`).
//!
//! 3 sync-state methods remain pending Phase 2b (sync orchestration
//! design) — `sync`, `get_sync_status`, `sync_async`. They need
//! engine-resident sync state per `domain_command.rs:307-313`.

use std::sync::Arc;

use super::VauchiPlatform;
use super::error::{MobileError, lock_or};
use super::types::{MobileSyncResult, MobileSyncStatus};

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

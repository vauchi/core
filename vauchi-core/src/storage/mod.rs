// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

mod connection;
mod device;

mod contact_row;

#[cfg(feature = "testing")]
pub mod contacts;
#[cfg(not(feature = "testing"))]
mod contacts;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

mod rekey;

#[cfg(feature = "testing")]
pub mod places;
#[cfg(not(feature = "testing"))]
mod places;

#[cfg(feature = "testing")]
pub mod activity_log;
#[cfg(not(feature = "testing"))]
mod activity_log;

pub mod local_keys;
pub mod migration;
pub mod secure;
mod stores;

pub use activity_log::ActivityLogRow;
pub use error::{
    DeletionState, DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord,
    DeviceDeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};
pub use stores::{
    ActivityLogStore, ConsentStore, ContactStore, DecoyStore, DeliveryStore, DeviceDeliveryStore,
    DeviceStore, DuressStore, EmergencyStore, FieldNoteStore, GENESIS_CONTACT_ATTEMPTS_PER_WINDOW,
    GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW, GENESIS_WINDOW_SECS, GenesisFactWrite, GenesisLimitStore,
    IdentityStore, LabelStore, OhttpCacheStore, PendingStore, PinCacheStore, PlaceStore,
    RatchetStore, RecoveryStore, ReplayStore, RetryStore, SafetyAlertFactStore,
    StoredSafetyAlertFact, SyncStore, TagStore, UxStore,
};

#[cfg(any(test, feature = "testing"))]
pub use secure::MemoryKeyStorage;

#[cfg(feature = "testing")]
pub use rekey::{ENCRYPTED_COLUMNS, REKEY_SKIP_COLUMNS};

#[cfg(feature = "secure-storage")]
pub use secure::PlatformKeyring;

use crate::crypto::SymmetricKey;
use rusqlite::Connection;

/// SQLite-based storage implementation.
///
/// Stores data in a local SQLite database with application-level encryption
/// for sensitive fields (keys, cards, etc.).
///
/// # Thread Safety (#80)
///
/// `Storage` is intentionally **not `Send`** because `rusqlite::Connection`
/// is not `Send`. Each client creates its own `Storage` instance on its
/// thread. For async contexts, wrap in `tokio::task::spawn_blocking` or use
/// a dedicated storage thread with a channel. The UniFFI mobile bindings
/// open a fresh storage per call via `open_vauchi()`.
pub struct Storage {
    conn: Connection,
    /// Encryption key derived from user's master key
    encryption_key: SymmetricKey,
    /// Database file path (None for in-memory databases).
    db_path: Option<std::path::PathBuf>,
    /// Explicit-time seam for the storage subsystem (Phase 1 /
    /// Task 1.1 of the pure-functional-core program). Each
    /// `save_*` / TTL-checking submodule uses `self.now_secs()`
    /// instead of reading ambient `SystemTime::now`. Defaults
    /// to `SystemClock::shared()`; tests inject a `FakeClock`
    /// via `with_clock(...)`.
    clock: std::sync::Arc<dyn crate::clock::Clock>,
    /// Test-only fault-injection latch. When armed, the next
    /// [`Storage::commit`] returns an error with the transaction left open,
    /// simulating a failed `COMMIT` so callers' rollback-on-commit-failure
    /// paths can be exercised deterministically. Absent in production builds.
    #[cfg(any(test, feature = "testing"))]
    commit_fault: std::cell::Cell<bool>,
}
impl Storage {
    /// Borrow the storage subsystem's [`Clock`](crate::clock::Clock).
    /// Default is `SystemClock::shared()`; tests inject via
    /// [`with_clock`](Self::with_clock).
    pub fn clock(&self) -> &std::sync::Arc<dyn crate::clock::Clock> {
        &self.clock
    }

    /// Replace the explicit-time seam. Used by `Vauchi` to wire its
    /// own clock into the storage subsystem so timestamps stamped
    /// inside `Storage::save_*` come from the same source as
    /// `Vauchi::clock().unix_seconds()`. Tests pass a `FakeClock`
    /// here.
    pub fn with_clock(mut self, clock: std::sync::Arc<dyn crate::clock::Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Current Unix-epoch seconds via the subsystem-owned [`Clock`].
    ///
    /// Used by submodules in `super::now_secs` (now retired) and by
    /// any `save_*` method that stamps `updated_at`. Equivalent to
    /// `self.clock.unix_seconds()`.
    pub(super) fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
}

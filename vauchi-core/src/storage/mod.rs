// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

#[cfg(feature = "network-rustls")]
mod backup_snapshot;
mod connection;
mod contact_move;
mod device;
pub(crate) mod duress_content;
mod key_check;
mod lookup;
mod mask;
mod mask_profile;
// INLINE_TEST_REQUIRED: a seal in the legacy decoy-table shape can only be built through the crate-private legacy writer, which no public API reaches any more
#[cfg(test)]
#[path = "mask_profile_tests.rs"]
mod mask_profile_tests;
mod sealing;
pub use sealing::Sealed;
mod receive_key;

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
pub(crate) mod migration_backup;
pub mod secure;
mod stores;

pub use activity_log::ActivityLogRow;
pub use error::{
    DeletionState, DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord,
    DeviceDeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, KeyAccess, SecureStorage};
pub use stores::RevokedSender;
pub use stores::{
    ActivityLogStore, ConsentStore, ContactStore, DecoyStore, DeliveryStore, DeviceDeliveryStore,
    DeviceStore, EmergencyStore, FieldNoteStore, GENESIS_CONTACT_ATTEMPTS_PER_WINDOW,
    GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW, GENESIS_WINDOW_SECS, GenesisFactWrite, GenesisLimitStore,
    GuardianBackupStore, IdentityStore, KeptPiece, LabelStore, MaskPublicValues,
    MaskPublicValuesStore, MaskSeed, MaskSeedStore, OhttpCacheStore, OwnCardStore, PendingStore,
    PinCacheStore, PlaceStore, RatchetStore, ReceiveCheckpoint, ReceiveScope, RecoveryStore,
    ReplayStore, RetryStore, SafetyAlertFactStore, StoredSafetyAlertFact, SyncStore, TagStore,
    UxStore,
};

#[cfg(any(test, feature = "testing"))]
pub use secure::MemoryKeyStorage;

pub use stores::ReceiveTokenStore;

pub use contact_move::{CONTACT_REFERENCES, ContactReference};
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
    /// The mask profile sealed in this file's `duress_content` row, for a
    /// real database (ADR-080); `None` for a mask profile itself.
    mask: Option<Box<Storage>>,
    /// Whether `mask` is lent to the foreground (ADR-080); `mask` then holds
    /// an empty stand-in that nothing seals.
    mask_lent: bool,
    /// Encryption key derived from user's master key
    encryption_key: SymmetricKey,
    /// The file holding this database sealed in one row, for a database that
    /// lives only in memory and in that seal (the mask profile, ADR-080).
    seal_target: Option<Connection>,
    /// The Background Receive Key, once the caller sets it: receive state
    /// is sealed under it so a locked phone can advance it (ADR-033
    /// amendment 2026-11-07).
    receive_key: Option<SymmetricKey>,
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

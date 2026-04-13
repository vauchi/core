// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

#[cfg(feature = "testing")]
pub mod consent;
#[cfg(not(feature = "testing"))]
mod consent;

mod connection;

mod contact_row;

#[cfg(feature = "testing")]
pub mod contact_ops;
#[cfg(not(feature = "testing"))]
mod contact_ops;

#[cfg(feature = "testing")]
pub mod contacts;
#[cfg(not(feature = "testing"))]
mod contacts;

#[cfg(feature = "testing")]
pub mod field_notes;
#[cfg(not(feature = "testing"))]
mod field_notes;

#[cfg(feature = "testing")]
pub mod device;
#[cfg(not(feature = "testing"))]
mod device;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod identity;
#[cfg(not(feature = "testing"))]
mod identity;

#[cfg(feature = "testing")]
pub mod labels;
#[cfg(not(feature = "testing"))]
mod labels;

#[cfg(feature = "testing")]
pub mod pending;
#[cfg(not(feature = "testing"))]
mod pending;

#[cfg(feature = "testing")]
pub mod delivery;
#[cfg(not(feature = "testing"))]
mod delivery;

#[cfg(feature = "testing")]
pub mod retry;
#[cfg(not(feature = "testing"))]
mod retry;

#[cfg(feature = "testing")]
pub mod device_delivery;
#[cfg(not(feature = "testing"))]
mod device_delivery;

#[cfg(feature = "testing")]
pub mod ratchet;
#[cfg(not(feature = "testing"))]
mod ratchet;

#[cfg(feature = "testing")]
pub mod recovery;
#[cfg(not(feature = "testing"))]
mod recovery;

mod rekey;

#[cfg(feature = "testing")]
pub mod local_groups;
#[cfg(not(feature = "testing"))]
mod local_groups;

#[cfg(feature = "testing")]
pub mod ux;
#[cfg(not(feature = "testing"))]
mod ux;

#[cfg(feature = "testing")]
pub mod decoy;
#[cfg(not(feature = "testing"))]
mod decoy;

#[cfg(feature = "testing")]
pub mod duress_settings;
#[cfg(not(feature = "testing"))]
mod duress_settings;

#[cfg(feature = "testing")]
pub mod emergency;
#[cfg(not(feature = "testing"))]
mod emergency;

#[cfg(feature = "testing")]
pub mod activity_log;
#[cfg(not(feature = "testing"))]
mod activity_log;

#[cfg(feature = "testing")]
pub mod exchange_state;
#[cfg(not(feature = "testing"))]
mod exchange_state;

mod contact_display_ops;
pub mod local_keys;
pub mod migration;
mod ohttp_cache;
mod pin_cache;
pub mod secure;

pub use activity_log::ActivityLogRow;
pub use error::{
    DeletionState, DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord,
    DeviceDeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};

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
    pub(super) encryption_key: SymmetricKey,
    /// Database file path (None for in-memory databases).
    db_path: Option<std::path::PathBuf>,
}

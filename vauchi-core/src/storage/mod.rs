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

#[cfg(feature = "testing")]
pub mod contacts;
#[cfg(not(feature = "testing"))]
mod contacts;

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
pub mod validation;
#[cfg(not(feature = "testing"))]
mod validation;

#[cfg(feature = "testing")]
pub mod recovery;
#[cfg(not(feature = "testing"))]
mod recovery;

#[cfg(feature = "testing")]
pub mod ux;
#[cfg(not(feature = "testing"))]
mod ux;

pub mod migration;
pub mod secure;

pub use error::{
    DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord, DeviceDeliveryStatus,
    OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError, UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};

#[cfg(feature = "secure-storage")]
pub use secure::PlatformKeyring;

use rusqlite::Connection;
use std::path::Path;

use crate::crypto::SymmetricKey;

/// SQLite-based storage implementation.
///
/// Stores data in a local SQLite database with application-level encryption
/// for sensitive fields (keys, cards, etc.).
pub struct Storage {
    conn: Connection,
    /// Encryption key derived from user's master key
    pub(crate) encryption_key: SymmetricKey,
}

impl Storage {
    /// Opens or creates a storage database at the given path.
    pub fn open<P: AsRef<Path>>(
        path: P,
        encryption_key: SymmetricKey,
    ) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let storage = Storage {
            conn,
            encryption_key,
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let storage = Storage {
            conn,
            encryption_key,
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Runs all pending schema migrations.
    fn run_migrations(&self) -> Result<(), StorageError> {
        let migrations = migration::all_migrations();
        migration::MigrationRunner::run(&self.conn, &self.encryption_key, &migrations)
    }

    /// Returns the current schema version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        migration::MigrationRunner::current_version(&self.conn)
    }
}

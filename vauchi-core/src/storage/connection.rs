// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Connection management, PRAGMA configuration, migrations, and core utilities.

use rusqlite::Connection;
use std::path::Path;

use crate::crypto::SymmetricKey;

use super::migration;
use super::{Storage, StorageError};

impl Storage {
    /// Opens or creates a storage database at the given path.
    pub fn open<P: AsRef<Path>>(
        path: P,
        encryption_key: SymmetricKey,
    ) -> Result<Self, StorageError> {
        let path_buf = path.as_ref().to_path_buf();
        let _is_new = !path_buf.exists();
        let conn = Connection::open(&path_buf)?;

        // Restrict database file permissions to owner-only (0600).
        // SQLite creates files with default umask (typically 0644), which
        // would make encrypted contact data world-readable on shared systems.
        #[cfg(unix)]
        if _is_new {
            use std::os::unix::fs::PermissionsExt;
            // best-effort: chmod can fail on FUSE/exotic filesystems where
            // POSIX permissions aren't supported; refusing to open would
            // break those users. Surface via tracing so it's visible in
            // logs without aborting the bring-up.
            if let Err(e) =
                std::fs::set_permissions(&path_buf, std::fs::Permissions::from_mode(0o600))
            {
                tracing::warn!(
                    target: "vauchi.storage.connection",
                    error = %e,
                    "set_permissions(0o600) on new database failed; file may be world-readable on shared systems"
                );
            }
        }

        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
            db_path: Some(path_buf),
            clock: crate::clock::SystemClock::shared(),
            #[cfg(any(test, feature = "testing"))]
            commit_fault: std::cell::Cell::new(false),
        };
        storage.run_migrations()?;
        // T2-12: Clean up old terminal delivery records on startup —
        // best-effort: maintenance failure leaves stale rows that will
        // be cleaned on the next startup; no data integrity impact
        #[allow(clippy::let_underscore_must_use)]
        let _ = storage.deliveries().run_startup_maintenance();
        // F4 audit fix: remove pre-migration .bak files after successful migration
        if let Some(ref db) = storage.db_path {
            Self::cleanup_migration_backups(db);
        }
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
            db_path: None,
            clock: crate::clock::SystemClock::shared(),
            #[cfg(any(test, feature = "testing"))]
            commit_fault: std::cell::Cell::new(false),
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Configures SQLite PRAGMAs for performance and security.
    ///
    /// Performance:
    /// - WAL mode: enables concurrent reads during writes
    /// - busy_timeout=5000: wait up to 5s for locks instead of failing immediately
    /// - synchronous=NORMAL: safe with WAL, better write throughput
    /// - cache_size=10000: larger page cache for query performance
    ///
    /// Security (defense-in-depth for crypto-shredding):
    /// - secure_delete=ON: overwrites deleted content with zeros
    /// - auto_vacuum=FULL: reclaims and overwrites freed pages on delete
    /// - temp_store=MEMORY: keeps temporary tables in RAM, not on disk
    ///
    /// Note: secure_delete is partially negated by WAL mode (pre-modification
    /// data persists in WAL file). The primary protection is the SMK encryption
    /// layer; these PRAGMAs are secondary defense-in-depth.
    fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
        // Set busy_timeout at the C level FIRST, before executing any SQL.
        // This ensures all subsequent statements (including the pragma batch
        // below) will wait up to 5s for locks instead of failing immediately.
        // Solves the bootstrap problem: SQL-based PRAGMA busy_timeout can
        // itself fail with SQLITE_BUSY if another connection holds a lock.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // auto_vacuum must be set before any tables are created (before first
        // page write), so it comes first — before journal_mode=WAL which writes
        // the database header.
        conn.execute_batch(
            "PRAGMA auto_vacuum=FULL;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=10000;
             PRAGMA secure_delete=ON;
             PRAGMA temp_store=MEMORY;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(())
    }

    /// Runs all pending schema migrations.
    ///
    /// For file-based databases, creates a pre-migration backup before
    /// applying pending migrations (#17).
    fn run_migrations(&self) -> Result<(), StorageError> {
        let migrations = migration::all_migrations();
        migration::MigrationRunner::run(
            &self.conn,
            &self.encryption_key,
            migrations,
            self.db_path.as_deref(),
            self.clock.unix_seconds(),
        )
    }

    /// Returns the current schema version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        migration::MigrationRunner::current_version(&self.conn)
    }

    /// Forces a WAL checkpoint, merging all WAL frames into the main DB file (#81).
    ///
    /// TRUNCATE mode flushes WAL contents and truncates the WAL file to zero bytes.
    /// This must be called before secure delete or rekey operations to ensure that
    /// pre-modification data in the WAL is merged and overwritable.
    pub fn wal_checkpoint(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| StorageError::Migration(format!("WAL checkpoint: {}", e)))
    }

    /// Begins a database transaction.
    ///
    /// Must be paired with `commit()` or `rollback()`.
    /// Use `BEGIN IMMEDIATE` to acquire a write lock immediately,
    /// preventing deadlocks when multiple writes are planned.
    pub fn begin_transaction(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE TRANSACTION")
            .map_err(StorageError::from)
    }

    /// Commits the current transaction.
    pub fn commit(&self) -> Result<(), StorageError> {
        // Test-only fault injection: return an error WITHOUT committing and
        // WITHOUT rolling back, so the transaction stays open exactly as a real
        // failed COMMIT leaves it — the caller must roll back or the next
        // `begin_transaction` wedges. Self-disarms after firing once.
        #[cfg(any(test, feature = "testing"))]
        if self.commit_fault.replace(false) {
            return Err(StorageError::Database(rusqlite::Error::QueryReturnedNoRows));
        }
        self.conn
            .execute_batch("COMMIT")
            .map_err(StorageError::from)
    }

    /// Arm a one-shot commit fault: the next [`Self::commit`] fails with the
    /// transaction left open. Only the explicit `commit()` path is affected;
    /// direct `execute_batch`/savepoint commits (e.g. the rate limiter) are
    /// not, so a fault targets the caller's own transaction precisely.
    #[cfg(any(test, feature = "testing"))]
    pub fn arm_commit_fault(&self) {
        self.commit_fault.set(true);
    }

    /// Rolls back the current transaction.
    pub fn rollback(&self) {
        // best-effort: rollback is called in Drop / failure paths; if
        // ROLLBACK itself fails the transaction is already gone with
        // the connection
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.conn.execute_batch("ROLLBACK");
    }

    /// Runs a fallible operation inside a nestable SQLite savepoint.
    pub(crate) fn with_savepoint<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<StorageError>,
    {
        self.conn
            .execute_batch("SAVEPOINT vauchi_atomic_operation")
            .map_err(StorageError::from)?;
        match operation() {
            Ok(value) => {
                self.conn
                    .execute_batch("RELEASE vauchi_atomic_operation")
                    .map_err(StorageError::from)?;
                Ok(value)
            }
            Err(error) => {
                self.conn
                    .execute_batch(
                        "ROLLBACK TO vauchi_atomic_operation;
                         RELEASE vauchi_atomic_operation;",
                    )
                    .map_err(StorageError::from)?;
                Err(error)
            }
        }
    }

    /// Removes `.pre-migration-v*.bak` files left by migration backups (F4 audit fix).
    ///
    /// Called on startup after migrations succeed. These backups are created by
    /// `VACUUM INTO` before migration runs; once migration commits, the backup
    /// is no longer needed and should not persist on disk.
    fn cleanup_migration_backups(db_path: &std::path::Path) {
        if let Some(dir) = db_path.parent()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.contains(".pre-migration-v") && name_str.ends_with(".bak") {
                    // best-effort: post-migration .bak cleanup; failure
                    // leaves a harmless backup file the user can remove
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// Returns a reference to the underlying connection (testing only).
    #[cfg(feature = "testing")]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Returns a reference to the storage encryption key (testing only).
    #[cfg(feature = "testing")]
    pub fn key(&self) -> &SymmetricKey {
        &self.encryption_key
    }
}

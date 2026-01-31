// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Database Schema Migration Framework
//!
//! Provides versioned schema migrations with transactional safety.
//! Each migration has a version number, name, and either SQL or a Rust callback.
//! The runner tracks applied versions in a `schema_version` table and runs
//! pending migrations in order within a single transaction.

use rusqlite::Connection;

use crate::crypto::SymmetricKey;

use super::StorageError;

/// A single schema migration step.
pub struct Migration {
    /// Monotonically increasing version number (starting at 1).
    pub version: u32,
    /// Human-readable name for this migration.
    pub name: &'static str,
    /// The migration action: either SQL or a Rust callback.
    pub action: MigrationAction,
}

/// The action a migration performs.
pub enum MigrationAction {
    /// Pure SQL migration.
    Sql(&'static str),
    /// Rust callback migration (for data transformations that need encryption key).
    Callback(fn(&Connection, &SymmetricKey) -> Result<(), StorageError>),
}

/// Runs schema migrations against a database connection.
pub struct MigrationRunner;

impl MigrationRunner {
    /// Runs all pending migrations in a transaction.
    ///
    /// Creates the `schema_version` table if it doesn't exist, then applies
    /// any migrations whose version is greater than the current schema version.
    /// All pending migrations run within a single transaction — if any migration
    /// fails, all changes are rolled back.
    pub fn run(
        conn: &Connection,
        key: &SymmetricKey,
        migrations: &[Migration],
    ) -> Result<(), StorageError> {
        // Create the schema_version table if it doesn't exist (outside transaction,
        // since we need to read it before starting the migration transaction).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            );",
        )?;

        let current_version = Self::current_version(conn)?;

        // Collect pending migrations
        let pending: Vec<&Migration> = migrations
            .iter()
            .filter(|m| m.version > current_version)
            .collect();

        if pending.is_empty() {
            return Ok(());
        }

        // Verify migrations are in order
        for window in pending.windows(2) {
            if window[0].version >= window[1].version {
                return Err(StorageError::Migration(format!(
                    "Migrations are not in order: v{} before v{}",
                    window[0].version, window[1].version
                )));
            }
        }

        // Run all pending migrations in a single transaction
        conn.execute_batch("BEGIN EXCLUSIVE TRANSACTION;")?;

        for migration in &pending {
            match &migration.action {
                MigrationAction::Sql(sql) => {
                    if let Err(e) = conn.execute_batch(sql) {
                        conn.execute_batch("ROLLBACK;")?;
                        return Err(StorageError::Migration(format!(
                            "Migration v{} '{}' failed: {}",
                            migration.version, migration.name, e
                        )));
                    }
                }
                MigrationAction::Callback(cb) => {
                    if let Err(e) = cb(conn, key) {
                        conn.execute_batch("ROLLBACK;")?;
                        return Err(StorageError::Migration(format!(
                            "Migration v{} '{}' callback failed: {}",
                            migration.version, migration.name, e
                        )));
                    }
                }
            }

            // Record this migration
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before UNIX epoch")
                .as_secs();

            if let Err(e) = conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![migration.version, now as i64],
            ) {
                conn.execute_batch("ROLLBACK;")?;
                return Err(StorageError::Migration(format!(
                    "Failed to record migration v{}: {}",
                    migration.version, e
                )));
            }
        }

        conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    /// Returns the current schema version, or 0 if no migrations have been applied.
    pub fn current_version(conn: &Connection) -> Result<u32, StorageError> {
        // Check if schema_version table exists
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |row| row.get(0),
        )?;

        if !table_exists {
            return Ok(0);
        }

        let version: Option<u32> = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .unwrap_or(None);

        Ok(version.unwrap_or(0))
    }
}

/// Returns all registered migrations in version order.
///
/// This is the single source of truth for the database schema.
/// New migrations are appended to the end of this list.
pub fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "baseline_schema",
            action: MigrationAction::Sql(MIGRATION_V1_BASELINE),
        },
        Migration {
            version: 2,
            name: "re_encrypt_aes_gcm_to_xchacha20",
            action: MigrationAction::Callback(migrate_v2_re_encrypt),
        },
        Migration {
            version: 3,
            name: "replay_nonces_table",
            action: MigrationAction::Sql(MIGRATION_V3_REPLAY_NONCES),
        },
        Migration {
            version: 4,
            name: "contact_enhancements",
            action: MigrationAction::Sql(MIGRATION_V4_CONTACT_ENHANCEMENTS),
        },
        Migration {
            version: 5,
            name: "gdpr_consent_audit",
            action: MigrationAction::Sql(MIGRATION_V5_GDPR_CONSENT),
        },
        Migration {
            version: 6,
            name: "device_sync_checkpoints",
            action: MigrationAction::Sql(MIGRATION_V6_DEVICE_CHECKPOINTS),
        },
        Migration {
            version: 7,
            name: "delivery_ttl_indexes",
            action: MigrationAction::Sql(MIGRATION_V7_DELIVERY_TTL),
        },
        Migration {
            version: 8,
            name: "recovery_tables",
            action: MigrationAction::Sql(MIGRATION_V8_RECOVERY),
        },
    ]
}

/// Migration v2: Re-encrypt all AES-GCM encrypted data to XChaCha20-Poly1305.
///
/// Reads each encrypted blob, decrypts with AES-GCM, re-encrypts with XChaCha20,
/// and writes it back. This is safe because the migration runs in a transaction.
fn migrate_v2_re_encrypt(conn: &Connection, key: &SymmetricKey) -> Result<(), StorageError> {
    use crate::crypto::{decrypt, encrypt};

    // Re-encrypt contacts: card_encrypted and shared_key_encrypted columns
    {
        let mut stmt = conn
            .prepare("SELECT id, card_encrypted, shared_key_encrypted FROM contacts")
            .map_err(|e| StorageError::Migration(format!("Failed to read contacts: {}", e)))?;

        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Failed to query contacts: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Failed to collect contacts: {}", e)))?;

        for (id, card_enc, key_enc) in &rows {
            // Decrypt with legacy format (handled by decrypt's auto-detect)
            let card_plain = decrypt(key, card_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt card for {}: {}", id, e)))?;
            let key_plain = decrypt(key, key_enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt shared_key for {}: {}", id, e))
            })?;

            // Re-encrypt with XChaCha20-Poly1305
            let card_new = encrypt(key, &card_plain).map_err(|e| {
                StorageError::Migration(format!("Re-encrypt card for {}: {}", id, e))
            })?;
            let key_new = encrypt(key, &key_plain).map_err(|e| {
                StorageError::Migration(format!("Re-encrypt shared_key for {}: {}", id, e))
            })?;

            conn.execute(
                "UPDATE contacts SET card_encrypted = ?1, shared_key_encrypted = ?2 WHERE id = ?3",
                rusqlite::params![card_new, key_new, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
        }
    }

    // Re-encrypt identity: backup_data_encrypted column
    {
        let result: Result<(i64, Vec<u8>), _> = conn.query_row(
            "SELECT id, backup_data_encrypted FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((id, backup_enc)) = result {
            let plain = decrypt(key, &backup_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt identity: {}", e)))?;
            let new_enc = encrypt(key, &plain)
                .map_err(|e| StorageError::Migration(format!("Re-encrypt identity: {}", e)))?;
            conn.execute(
                "UPDATE identity SET backup_data_encrypted = ?1 WHERE id = ?2",
                rusqlite::params![new_enc, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update identity: {}", e)))?;
        }
    }

    // Re-encrypt ratchet state: ratchet_state_encrypted column
    {
        let mut stmt = conn
            .prepare("SELECT contact_id, ratchet_state_encrypted FROM contact_ratchets")
            .map_err(|e| StorageError::Migration(format!("Failed to read ratchets: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Failed to query ratchets: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Failed to collect ratchets: {}", e)))?;

        for (contact_id, ratchet_enc) in &rows {
            let plain = decrypt(key, ratchet_enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt ratchet for {}: {}", contact_id, e))
            })?;
            let new_enc = encrypt(key, &plain).map_err(|e| {
                StorageError::Migration(format!("Re-encrypt ratchet for {}: {}", contact_id, e))
            })?;
            conn.execute(
                "UPDATE contact_ratchets SET ratchet_state_encrypted = ?1 WHERE contact_id = ?2",
                rusqlite::params![new_enc, contact_id],
            )
            .map_err(|e| {
                StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e))
            })?;
        }
    }

    Ok(())
}

/// Migration v1: Baseline schema.
///
/// This captures the entire original schema as the first migration.
/// Existing databases that were created before the migration framework
/// will already have these tables (via CREATE TABLE IF NOT EXISTS),
/// so this migration is safe to run on both new and existing databases.
const MIGRATION_V1_BASELINE: &str = "
    -- Contacts table
    CREATE TABLE IF NOT EXISTS contacts (
        id TEXT PRIMARY KEY,
        public_key BLOB NOT NULL,
        display_name TEXT NOT NULL,
        card_encrypted BLOB NOT NULL,
        shared_key_encrypted BLOB NOT NULL,
        visibility_rules_json TEXT,
        exchange_timestamp INTEGER NOT NULL,
        fingerprint_verified INTEGER DEFAULT 0,
        last_sync_at INTEGER
    );

    -- Own contact card
    CREATE TABLE IF NOT EXISTS own_card (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        card_json TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Identity (encrypted backup data)
    CREATE TABLE IF NOT EXISTS identity (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        backup_data_encrypted BLOB NOT NULL,
        display_name TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );

    -- Pending sync updates
    CREATE TABLE IF NOT EXISTS pending_updates (
        id TEXT PRIMARY KEY,
        contact_id TEXT NOT NULL,
        update_type TEXT NOT NULL,
        payload BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        retry_count INTEGER DEFAULT 0,
        status TEXT DEFAULT 'pending',
        error_message TEXT,
        retry_at INTEGER
    );

    -- Contact sync timestamps
    CREATE TABLE IF NOT EXISTS contact_sync_timestamps (
        contact_id TEXT PRIMARY KEY,
        last_sync_at INTEGER NOT NULL
    );

    -- Double Ratchet state for each contact
    CREATE TABLE IF NOT EXISTS contact_ratchets (
        contact_id TEXT PRIMARY KEY REFERENCES contacts(id),
        ratchet_state_encrypted BLOB NOT NULL,
        is_initiator INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Device info (current device)
    CREATE TABLE IF NOT EXISTS device_info (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        device_id BLOB NOT NULL,
        device_index INTEGER NOT NULL,
        device_name TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );

    -- Device registry (all linked devices)
    CREATE TABLE IF NOT EXISTS device_registry (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        registry_json TEXT NOT NULL,
        version INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Inter-device sync state
    CREATE TABLE IF NOT EXISTS device_sync_state (
        device_id BLOB PRIMARY KEY,
        state_json TEXT NOT NULL,
        last_sync_version INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Local version vector for causality tracking
    CREATE TABLE IF NOT EXISTS version_vector (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        vector_json TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Visibility labels
    CREATE TABLE IF NOT EXISTS visibility_labels (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        contacts_json TEXT NOT NULL DEFAULT '[]',
        visible_fields_json TEXT NOT NULL DEFAULT '[]',
        created_at INTEGER NOT NULL,
        modified_at INTEGER NOT NULL
    );

    -- Per-contact visibility overrides
    CREATE TABLE IF NOT EXISTS contact_visibility_overrides (
        contact_id TEXT NOT NULL,
        field_id TEXT NOT NULL,
        is_visible INTEGER NOT NULL,
        PRIMARY KEY (contact_id, field_id)
    );

    -- Delivery records (outbound message delivery tracking)
    CREATE TABLE IF NOT EXISTS delivery_records (
        message_id TEXT PRIMARY KEY,
        recipient_id TEXT NOT NULL,
        status TEXT NOT NULL,
        status_reason TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        expires_at INTEGER
    );

    -- Retry queue (failed deliveries awaiting retry)
    CREATE TABLE IF NOT EXISTS retry_entries (
        message_id TEXT PRIMARY KEY,
        recipient_id TEXT NOT NULL,
        payload BLOB NOT NULL,
        attempt INTEGER NOT NULL DEFAULT 0,
        next_retry INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        max_attempts INTEGER NOT NULL DEFAULT 10
    );

    -- Per-device delivery tracking
    CREATE TABLE IF NOT EXISTS device_deliveries (
        message_id TEXT NOT NULL,
        device_id TEXT NOT NULL,
        recipient_id TEXT NOT NULL,
        status TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (message_id, device_id)
    );

    -- Field validations (crowd-sourced verification)
    CREATE TABLE IF NOT EXISTS field_validations (
        id TEXT PRIMARY KEY,
        contact_id TEXT NOT NULL,
        field_id TEXT NOT NULL,
        field_value TEXT NOT NULL,
        validator_id TEXT NOT NULL,
        validated_at INTEGER NOT NULL,
        signature BLOB NOT NULL,
        UNIQUE(contact_id, field_id, validator_id)
    );

    -- User experience state (aha moments, demo contact)
    CREATE TABLE IF NOT EXISTS ux_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        aha_tracker_json TEXT,
        demo_contact_json TEXT,
        updated_at INTEGER NOT NULL
    );

    -- Indexes
    CREATE INDEX IF NOT EXISTS idx_pending_contact ON pending_updates(contact_id);
    CREATE INDEX IF NOT EXISTS idx_pending_status ON pending_updates(status);
    CREATE INDEX IF NOT EXISTS idx_label_name ON visibility_labels(name);
    CREATE INDEX IF NOT EXISTS idx_delivery_recipient ON delivery_records(recipient_id);
    CREATE INDEX IF NOT EXISTS idx_delivery_status ON delivery_records(status);
    CREATE INDEX IF NOT EXISTS idx_retry_next ON retry_entries(next_retry);
    CREATE INDEX IF NOT EXISTS idx_retry_recipient ON retry_entries(recipient_id);
    CREATE INDEX IF NOT EXISTS idx_device_delivery_message ON device_deliveries(message_id);
    CREATE INDEX IF NOT EXISTS idx_device_delivery_status ON device_deliveries(status);
    CREATE INDEX IF NOT EXISTS idx_validation_contact ON field_validations(contact_id);
    CREATE INDEX IF NOT EXISTS idx_validation_field ON field_validations(contact_id, field_id);
    CREATE INDEX IF NOT EXISTS idx_validation_validator ON field_validations(validator_id);
";

/// Migration v3: Replay nonces table for replay attack detection.
const MIGRATION_V3_REPLAY_NONCES: &str = "
    CREATE TABLE IF NOT EXISTS replay_nonces (
        contact_id TEXT NOT NULL,
        nonce BLOB NOT NULL,
        timestamp INTEGER NOT NULL,
        PRIMARY KEY (contact_id, nonce)
    );

    CREATE INDEX IF NOT EXISTS idx_replay_timestamp ON replay_nonces(timestamp);
";

/// Migration v4: Contact enhancements — blocked/hidden/favorite persistence,
/// personal notes, avatar, contact limits.
const MIGRATION_V4_CONTACT_ENHANCEMENTS: &str = "
    ALTER TABLE contacts ADD COLUMN blocked INTEGER DEFAULT 0;
    ALTER TABLE contacts ADD COLUMN hidden INTEGER DEFAULT 0;
    ALTER TABLE contacts ADD COLUMN favorite INTEGER DEFAULT 0;
    ALTER TABLE contacts ADD COLUMN personal_notes_encrypted BLOB;
    ALTER TABLE contacts ADD COLUMN avatar_encrypted BLOB;

    CREATE TABLE IF NOT EXISTS contact_limits (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        max_contacts INTEGER DEFAULT 500
    );

    INSERT OR IGNORE INTO contact_limits (id, max_contacts) VALUES (1, 500);
";

/// Migration v5: GDPR consent records and audit log.
const MIGRATION_V5_GDPR_CONSENT: &str = "
    CREATE TABLE IF NOT EXISTS consent_records (
        id TEXT PRIMARY KEY,
        consent_type TEXT NOT NULL,
        granted INTEGER NOT NULL,
        timestamp INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        event_type TEXT NOT NULL,
        details TEXT,
        timestamp INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
    CREATE INDEX IF NOT EXISTS idx_audit_event_type ON audit_log(event_type);
";

/// Migration v6: Device sync checkpoints for interrupted sync resume.
const MIGRATION_V6_DEVICE_CHECKPOINTS: &str = "
    CREATE TABLE IF NOT EXISTS device_sync_checkpoints (
        target_device_id BLOB PRIMARY KEY,
        items_json TEXT NOT NULL,
        sent_count INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v7: Delivery TTL indexes for efficient expiry queries.
const MIGRATION_V7_DELIVERY_TTL: &str = "
    CREATE INDEX IF NOT EXISTS idx_delivery_expires ON delivery_records(expires_at)
        WHERE expires_at IS NOT NULL;
";

/// Migration v8: Recovery response and rate limit tables.
const MIGRATION_V8_RECOVERY: &str = "
    CREATE TABLE IF NOT EXISTS recovery_responses (
        claim_id TEXT PRIMARY KEY,
        contact_id TEXT NOT NULL,
        response TEXT NOT NULL,
        remind_at INTEGER,
        created_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS recovery_rate_limits (
        identity_pk BLOB PRIMARY KEY,
        claim_count INTEGER NOT NULL DEFAULT 0,
        window_start INTEGER NOT NULL
    );
";

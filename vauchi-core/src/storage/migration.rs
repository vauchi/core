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
        Migration {
            version: 9,
            name: "recovery_trust",
            action: MigrationAction::Sql(MIGRATION_V9_RECOVERY_TRUST),
        },
        Migration {
            version: 10,
            name: "gdpr_deletion_consent_versioning",
            action: MigrationAction::Sql(MIGRATION_V10_GDPR_ENHANCEMENTS),
        },
        Migration {
            version: 11,
            name: "sync_checkpoints_atomic",
            action: MigrationAction::Sql(MIGRATION_V11_SYNC_CHECKPOINTS),
        },
        Migration {
            version: 12,
            name: "contacts_display_name_index",
            action: MigrationAction::Sql(MIGRATION_V12_CONTACTS_INDEX),
        },
        Migration {
            version: 13,
            name: "crypto_shredding",
            action: MigrationAction::Sql(MIGRATION_V13_CRYPTO_SHREDDING),
        },
        Migration {
            version: 14,
            name: "encrypt_high_priority_tables",
            action: MigrationAction::Callback(migrate_v14_encrypt_high_priority),
        },
        Migration {
            version: 15,
            name: "encrypt_medium_priority_tables",
            action: MigrationAction::Callback(migrate_v15_encrypt_medium_priority),
        },
        Migration {
            version: 16,
            name: "encrypt_low_priority_tables",
            action: MigrationAction::Callback(migrate_v16_encrypt_low_priority),
        },
        Migration {
            version: 17,
            name: "tor_config_column",
            action: MigrationAction::Sql(MIGRATION_V17_TOR_CONFIG),
        },
        Migration {
            version: 18,
            name: "encrypt_visibility_rules",
            action: MigrationAction::Callback(migrate_v18_encrypt_visibility_rules),
        },
    ]
}

/// Migration v17: Add tor_config_encrypted column to ux_state table.
///
/// Stores encrypted Tor configuration (enabled, bridges, preferences)
/// alongside other UX state.
const MIGRATION_V17_TOR_CONFIG: &str = "
    ALTER TABLE ux_state ADD COLUMN tor_config_encrypted BLOB;
";

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

/// Migration v6: Per-device sync checkpoints for interrupted sync resume.
///
/// Tracks sync progress per target device — stores a serialized list of SyncItems
/// and a sent_count so sync can resume from the last sent item after interruption.
/// Keyed by `target_device_id`. See also V11 (`sync_checkpoints`) which tracks
/// batch-level progress for crash recovery across all sync operations.
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

/// Migration v9: Add recovery_trusted column to contacts.
///
/// Existing contacts default to not trusted — users must explicitly
/// opt in to mark contacts as trusted for recovery.
const MIGRATION_V9_RECOVERY_TRUST: &str = "
    ALTER TABLE contacts ADD COLUMN recovery_trusted INTEGER DEFAULT 0;
";

/// Migration v10: GDPR enhancements — deletion state table, consent versioning.
const MIGRATION_V10_GDPR_ENHANCEMENTS: &str = "
    CREATE TABLE IF NOT EXISTS deletion_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        state_json TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- Add policy_version column to consent_records (nullable for backward compat)
    ALTER TABLE consent_records ADD COLUMN policy_version TEXT;
";

/// Migration v11: Batch-level sync checkpoints for crash recovery.
///
/// Tracks progress of multi-item sync batches (total_items, processed_items)
/// so a batch can be resumed after a crash. Keyed by `checkpoint_id` with a
/// `batch_id` index. Distinct from V6 (`device_sync_checkpoints`) which tracks
/// per-device sync progress with serialized SyncItem lists.
const MIGRATION_V11_SYNC_CHECKPOINTS: &str = "
    CREATE TABLE IF NOT EXISTS sync_checkpoints (
        checkpoint_id TEXT PRIMARY KEY,
        batch_id TEXT NOT NULL,
        total_items INTEGER NOT NULL,
        processed_items INTEGER NOT NULL,
        state_json TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_checkpoint_batch ON sync_checkpoints(batch_id);
";

/// Migration v12: Case-insensitive index on contacts.display_name for search performance.
const MIGRATION_V12_CONTACTS_INDEX: &str = "
    CREATE INDEX IF NOT EXISTS idx_contacts_display_name
        ON contacts(display_name COLLATE NOCASE);
";

/// Migration v13: Crypto-shredding support.
///
/// Adds per-contact Content Encryption Key (CEK) column and revoked_senders
/// tombstone table. Existing contacts have `cek_encrypted = NULL` (legacy mode)
/// until the card owner sends an update carrying a CEK.
///
/// ## GDPR note: `revoked_senders` retention (Art 6(1)(f))
///
/// The `revoked_senders` table persists indefinitely by design. This is
/// defensible under GDPR Art 6(1)(f) — legitimate interest:
///
/// - **Purpose**: Prevents re-establishment attacks from revoked senders.
///   Without tombstones, a revoked sender could forge a new exchange and
///   re-appear as a trusted contact.
/// - **Minimal data**: Only `sender_id` (a hash of the public key, not PII)
///   and `revoked_at` timestamp are stored. No names, messages, or content.
/// - **Full deletion path**: Account shredding (hard_shred / panic_shred)
///   destroys the entire SQLite database including all tombstones. Users
///   who exercise their right to erasure (Art 17) get complete deletion.
const MIGRATION_V13_CRYPTO_SHREDDING: &str = "
    -- Per-contact CEK, encrypted with the storage master key.
    -- NULL for legacy contacts (pre-CEK). Non-NULL for CEK-protected contacts.
    ALTER TABLE contacts ADD COLUMN cek_encrypted BLOB;

    -- Revocation tombstones: prevents processing updates from revoked senders.
    -- Persists indefinitely to prevent re-establishment attacks.
    -- GDPR: Art 6(1)(f) legitimate interest — see doc comment above.
    CREATE TABLE IF NOT EXISTS revoked_senders (
        sender_id TEXT PRIMARY KEY,
        revoked_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_revoked_at ON revoked_senders(revoked_at);
";

/// Migration v14: Add encrypted columns for high-priority plaintext tables.
///
/// Adds `_encrypted` BLOB columns to: own_card, device_registry, device_sync_state,
/// and visibility_labels. Existing plaintext data is encrypted and stored in the
/// new columns. The old plaintext columns are kept for backward compatibility but
/// cleared to empty strings.
fn migrate_v14_encrypt_high_priority(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::encrypt;

    // Step 1: Add encrypted columns to each table
    conn.execute_batch(
        "ALTER TABLE own_card ADD COLUMN card_json_encrypted BLOB;
         ALTER TABLE device_registry ADD COLUMN registry_json_encrypted BLOB;
         ALTER TABLE device_sync_state ADD COLUMN state_json_encrypted BLOB;
         ALTER TABLE visibility_labels ADD COLUMN contacts_json_encrypted BLOB;
         ALTER TABLE visibility_labels ADD COLUMN visible_fields_json_encrypted BLOB;",
    )
    .map_err(|e| StorageError::Migration(format!("Failed to add encrypted columns: {}", e)))?;

    // Step 2: Encrypt existing plaintext data in own_card
    {
        let result: Result<(String,), _> =
            conn.query_row("SELECT card_json FROM own_card WHERE id = 1", [], |row| {
                Ok((row.get(0)?,))
            });

        if let Ok((card_json,)) = result {
            let encrypted = encrypt(key, card_json.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt own_card: {}", e)))?;
            conn.execute(
                "UPDATE own_card SET card_json_encrypted = ?1, card_json = '' WHERE id = 1",
                rusqlite::params![encrypted],
            )
            .map_err(|e| StorageError::Migration(format!("Update own_card: {}", e)))?;
        }
    }

    // Step 3: Encrypt existing plaintext data in device_registry
    {
        let result: Result<(String,), _> = conn.query_row(
            "SELECT registry_json FROM device_registry WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((registry_json,)) = result {
            let encrypted = encrypt(key, registry_json.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt device_registry: {}", e)))?;
            conn.execute(
                "UPDATE device_registry SET registry_json_encrypted = ?1, registry_json = '' WHERE id = 1",
                rusqlite::params![encrypted],
            )
            .map_err(|e| StorageError::Migration(format!("Update device_registry: {}", e)))?;
        }
    }

    // Step 4: Encrypt existing plaintext data in device_sync_state
    {
        let mut stmt = conn
            .prepare("SELECT device_id, state_json FROM device_sync_state")
            .map_err(|e| {
                StorageError::Migration(format!("Failed to read device_sync_state: {}", e))
            })?;

        let rows: Vec<(Vec<u8>, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| {
                StorageError::Migration(format!("Failed to query device_sync_state: {}", e))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::Migration(format!("Failed to collect device_sync_state: {}", e))
            })?;

        for (device_id, state_json) in &rows {
            let encrypted = encrypt(key, state_json.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt device_sync_state: {}", e))
            })?;
            conn.execute(
                "UPDATE device_sync_state SET state_json_encrypted = ?1, state_json = '' WHERE device_id = ?2",
                rusqlite::params![encrypted, device_id],
            )
            .map_err(|e| {
                StorageError::Migration(format!("Update device_sync_state: {}", e))
            })?;
        }
    }

    // Step 5: Encrypt existing plaintext data in visibility_labels
    {
        let mut stmt = conn
            .prepare("SELECT id, contacts_json, visible_fields_json FROM visibility_labels")
            .map_err(|e| {
                StorageError::Migration(format!("Failed to read visibility_labels: {}", e))
            })?;

        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| {
                StorageError::Migration(format!("Failed to query visibility_labels: {}", e))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::Migration(format!("Failed to collect visibility_labels: {}", e))
            })?;

        for (id, contacts_json, fields_json) in &rows {
            let contacts_enc = encrypt(key, contacts_json.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt visibility_labels contacts: {}", e))
            })?;
            let fields_enc = encrypt(key, fields_json.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt visibility_labels fields: {}", e))
            })?;
            conn.execute(
                "UPDATE visibility_labels SET contacts_json_encrypted = ?1, visible_fields_json_encrypted = ?2, contacts_json = '[]', visible_fields_json = '[]' WHERE id = ?3",
                rusqlite::params![contacts_enc, fields_enc, id],
            )
            .map_err(|e| {
                StorageError::Migration(format!("Update visibility_labels: {}", e))
            })?;
        }
    }

    Ok(())
}

/// Migration v15: Add encrypted columns for medium-priority plaintext tables.
///
/// Adds `_encrypted` BLOB columns to: device_info, version_vector,
/// contact_sync_timestamps, pending_updates, retry_entries,
/// device_sync_checkpoints, recovery_responses, deletion_state, and
/// sync_checkpoints. Existing plaintext data is encrypted and stored
/// in the new columns. Old plaintext columns are cleared.
fn migrate_v15_encrypt_medium_priority(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::encrypt;

    // Step 1: Add encrypted columns to each table
    conn.execute_batch(
        "ALTER TABLE device_info ADD COLUMN device_info_encrypted BLOB;
         ALTER TABLE version_vector ADD COLUMN vector_json_encrypted BLOB;
         ALTER TABLE contact_sync_timestamps ADD COLUMN last_sync_at_encrypted BLOB;
         ALTER TABLE pending_updates ADD COLUMN payload_encrypted BLOB;
         ALTER TABLE retry_entries ADD COLUMN payload_encrypted BLOB;
         ALTER TABLE device_sync_checkpoints ADD COLUMN items_json_encrypted BLOB;
         ALTER TABLE recovery_responses ADD COLUMN response_encrypted BLOB;
         ALTER TABLE deletion_state ADD COLUMN state_json_encrypted BLOB;
         ALTER TABLE sync_checkpoints ADD COLUMN state_json_encrypted BLOB;",
    )
    .map_err(|e| StorageError::Migration(format!("Failed to add v15 encrypted columns: {}", e)))?;

    // Step 2: Encrypt existing data in device_info
    {
        let result: Result<(Vec<u8>, i32, String, i64), _> = conn.query_row(
            "SELECT device_id, device_index, device_name, created_at FROM device_info WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        );

        if let Ok((device_id, device_index, device_name, created_at)) = result {
            let json = serde_json::json!({
                "device_id": device_id,
                "device_index": device_index,
                "device_name": device_name,
                "created_at": created_at,
            });
            let json_bytes = serde_json::to_vec(&json)
                .map_err(|e| StorageError::Migration(format!("Serialize device_info: {}", e)))?;
            let encrypted = encrypt(key, &json_bytes)
                .map_err(|e| StorageError::Migration(format!("Encrypt device_info: {}", e)))?;
            conn.execute(
                "UPDATE device_info SET device_info_encrypted = ?1, device_name = '' WHERE id = 1",
                rusqlite::params![encrypted],
            )
            .map_err(|e| StorageError::Migration(format!("Update device_info: {}", e)))?;
        }
    }

    // Step 3: Encrypt existing data in version_vector
    {
        let result: Result<(String,), _> = conn.query_row(
            "SELECT vector_json FROM version_vector WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((vector_json,)) = result {
            let encrypted = encrypt(key, vector_json.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt version_vector: {}", e)))?;
            conn.execute(
                "UPDATE version_vector SET vector_json_encrypted = ?1, vector_json = '' WHERE id = 1",
                rusqlite::params![encrypted],
            )
            .map_err(|e| StorageError::Migration(format!("Update version_vector: {}", e)))?;
        }
    }

    // Step 4: Encrypt existing data in contact_sync_timestamps
    {
        let mut stmt = conn
            .prepare("SELECT contact_id, last_sync_at FROM contact_sync_timestamps")
            .map_err(|e| StorageError::Migration(format!("Read contact_sync_timestamps: {}", e)))?;

        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact_sync_timestamps: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::Migration(format!("Collect contact_sync_timestamps: {}", e))
            })?;

        for (contact_id, last_sync_at) in &rows {
            let ts_bytes = last_sync_at.to_le_bytes();
            let encrypted = encrypt(key, &ts_bytes).map_err(|e| {
                StorageError::Migration(format!("Encrypt contact_sync_timestamps: {}", e))
            })?;
            conn.execute(
                "UPDATE contact_sync_timestamps SET last_sync_at_encrypted = ?1 WHERE contact_id = ?2",
                rusqlite::params![encrypted, contact_id],
            )
            .map_err(|e| StorageError::Migration(format!("Update contact_sync_timestamps: {}", e)))?;
        }
    }

    // Step 5: Encrypt existing data in pending_updates
    {
        let mut stmt = conn
            .prepare("SELECT id, payload FROM pending_updates")
            .map_err(|e| StorageError::Migration(format!("Read pending_updates: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query pending_updates: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect pending_updates: {}", e)))?;

        for (id, payload) in &rows {
            let encrypted = encrypt(key, payload)
                .map_err(|e| StorageError::Migration(format!("Encrypt pending_updates: {}", e)))?;
            conn.execute(
                "UPDATE pending_updates SET payload_encrypted = ?1, payload = X'' WHERE id = ?2",
                rusqlite::params![encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update pending_updates: {}", e)))?;
        }
    }

    // Step 6: Encrypt existing data in retry_entries
    {
        let mut stmt = conn
            .prepare("SELECT message_id, payload FROM retry_entries")
            .map_err(|e| StorageError::Migration(format!("Read retry_entries: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query retry_entries: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect retry_entries: {}", e)))?;

        for (message_id, payload) in &rows {
            let encrypted = encrypt(key, payload)
                .map_err(|e| StorageError::Migration(format!("Encrypt retry_entries: {}", e)))?;
            conn.execute(
                "UPDATE retry_entries SET payload_encrypted = ?1, payload = X'' WHERE message_id = ?2",
                rusqlite::params![encrypted, message_id],
            )
            .map_err(|e| StorageError::Migration(format!("Update retry_entries: {}", e)))?;
        }
    }

    // Step 7: Encrypt existing data in device_sync_checkpoints
    {
        let mut stmt = conn
            .prepare("SELECT target_device_id, items_json FROM device_sync_checkpoints")
            .map_err(|e| StorageError::Migration(format!("Read device_sync_checkpoints: {}", e)))?;

        let rows: Vec<(Vec<u8>, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query device_sync_checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::Migration(format!("Collect device_sync_checkpoints: {}", e))
            })?;

        for (target_device_id, items_json) in &rows {
            let encrypted = encrypt(key, items_json.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt device_sync_checkpoints: {}", e))
            })?;
            conn.execute(
                "UPDATE device_sync_checkpoints SET items_json_encrypted = ?1, items_json = '' WHERE target_device_id = ?2",
                rusqlite::params![encrypted, target_device_id],
            )
            .map_err(|e| StorageError::Migration(format!("Update device_sync_checkpoints: {}", e)))?;
        }
    }

    // Step 8: Encrypt existing data in recovery_responses
    {
        let mut stmt = conn
            .prepare("SELECT claim_id, response FROM recovery_responses")
            .map_err(|e| StorageError::Migration(format!("Read recovery_responses: {}", e)))?;

        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query recovery_responses: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect recovery_responses: {}", e)))?;

        for (claim_id, response) in &rows {
            let encrypted = encrypt(key, response.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt recovery_responses: {}", e))
            })?;
            conn.execute(
                "UPDATE recovery_responses SET response_encrypted = ?1, response = '' WHERE claim_id = ?2",
                rusqlite::params![encrypted, claim_id],
            )
            .map_err(|e| StorageError::Migration(format!("Update recovery_responses: {}", e)))?;
        }
    }

    // Step 9: Encrypt existing data in deletion_state
    {
        let result: Result<(String,), _> = conn.query_row(
            "SELECT state_json FROM deletion_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((state_json,)) = result {
            let encrypted = encrypt(key, state_json.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt deletion_state: {}", e)))?;
            conn.execute(
                "UPDATE deletion_state SET state_json_encrypted = ?1, state_json = '' WHERE id = 1",
                rusqlite::params![encrypted],
            )
            .map_err(|e| StorageError::Migration(format!("Update deletion_state: {}", e)))?;
        }
    }

    // Step 10: Encrypt existing data in sync_checkpoints
    {
        let mut stmt = conn
            .prepare("SELECT checkpoint_id, state_json FROM sync_checkpoints")
            .map_err(|e| StorageError::Migration(format!("Read sync_checkpoints: {}", e)))?;

        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query sync_checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect sync_checkpoints: {}", e)))?;

        for (checkpoint_id, state_json) in &rows {
            let encrypted = encrypt(key, state_json.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt sync_checkpoints: {}", e)))?;
            conn.execute(
                "UPDATE sync_checkpoints SET state_json_encrypted = ?1, state_json = '' WHERE checkpoint_id = ?2",
                rusqlite::params![encrypted, checkpoint_id],
            )
            .map_err(|e| StorageError::Migration(format!("Update sync_checkpoints: {}", e)))?;
        }
    }

    Ok(())
}

/// Migration v16: Add encrypted columns for low-priority plaintext tables.
///
/// Tables encrypted: field_validations (field_value, signature),
/// ux_state (aha_tracker_json, demo_contact_json), audit_log (details).
///
/// Tables skipped:
/// - replay_nonces: contains only random nonces + timestamps, no personal data
/// - consent_records: consent decisions aren't personal data; needed for queries
fn migrate_v16_encrypt_low_priority(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::encrypt;

    // Step 1: Add encrypted columns
    conn.execute_batch(
        "ALTER TABLE field_validations ADD COLUMN field_value_encrypted BLOB;
         ALTER TABLE field_validations ADD COLUMN signature_encrypted BLOB;
         ALTER TABLE ux_state ADD COLUMN aha_tracker_json_encrypted BLOB;
         ALTER TABLE ux_state ADD COLUMN demo_contact_json_encrypted BLOB;
         ALTER TABLE audit_log ADD COLUMN details_encrypted BLOB;",
    )
    .map_err(|e| StorageError::Migration(format!("Add v16 columns: {}", e)))?;

    // Step 2: Encrypt existing field_validations data
    {
        let mut stmt = conn
            .prepare("SELECT id, field_value, signature FROM field_validations")
            .map_err(|e| StorageError::Migration(format!("Read field_validations: {}", e)))?;
        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query field_validations: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect field_validations: {}", e)))?;

        for (id, field_value, signature) in &rows {
            let fv_encrypted = encrypt(key, field_value.as_bytes())
                .map_err(|e| StorageError::Migration(format!("Encrypt field_value: {}", e)))?;
            let sig_encrypted = encrypt(key, signature)
                .map_err(|e| StorageError::Migration(format!("Encrypt signature: {}", e)))?;
            conn.execute(
                "UPDATE field_validations SET field_value_encrypted = ?1, field_value = '', signature_encrypted = ?2, signature = X'' WHERE id = ?3",
                rusqlite::params![fv_encrypted, sig_encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update field_validations: {}", e)))?;
        }
    }

    // Step 3: Encrypt existing ux_state data
    {
        let result = conn.query_row(
            "SELECT id, aha_tracker_json, demo_contact_json FROM ux_state WHERE id = 1",
            [],
            |row| {
                let id: i64 = row.get(0)?;
                let aha: Option<String> = row.get(1)?;
                let demo: Option<String> = row.get(2)?;
                Ok((id, aha, demo))
            },
        );

        if let Ok((id, aha_json, demo_json)) = result {
            let aha_encrypted = if let Some(ref json) = aha_json {
                if !json.is_empty() {
                    Some(encrypt(key, json.as_bytes()).map_err(|e| {
                        StorageError::Migration(format!("Encrypt aha_tracker: {}", e))
                    })?)
                } else {
                    None
                }
            } else {
                None
            };

            let demo_encrypted = if let Some(ref json) = demo_json {
                if !json.is_empty() {
                    Some(encrypt(key, json.as_bytes()).map_err(|e| {
                        StorageError::Migration(format!("Encrypt demo_contact: {}", e))
                    })?)
                } else {
                    None
                }
            } else {
                None
            };

            conn.execute(
                "UPDATE ux_state SET aha_tracker_json_encrypted = ?1, aha_tracker_json = '', demo_contact_json_encrypted = ?2, demo_contact_json = '' WHERE id = ?3",
                rusqlite::params![aha_encrypted, demo_encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update ux_state: {}", e)))?;
        }
    }

    // Step 4: Encrypt existing audit_log details
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, details FROM audit_log WHERE details IS NOT NULL AND details != ''",
            )
            .map_err(|e| StorageError::Migration(format!("Read audit_log: {}", e)))?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query audit_log: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect audit_log: {}", e)))?;

        for (id, details) in &rows {
            let encrypted = encrypt(key, details.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt audit_log details: {}", e))
            })?;
            conn.execute(
                "UPDATE audit_log SET details_encrypted = ?1, details = '' WHERE id = ?2",
                rusqlite::params![encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update audit_log: {}", e)))?;
        }
    }

    Ok(())
}

/// Migration v18: Encrypt visibility_rules column in contacts table.
///
/// The `visibility_rules_json` column was the only unencrypted personal data
/// field in the contacts table. This migration adds an encrypted column,
/// encrypts all existing plaintext rules, and clears the plaintext.
fn migrate_v18_encrypt_visibility_rules(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::encrypt;

    // 1. Add new encrypted column
    conn.execute_batch("ALTER TABLE contacts ADD COLUMN visibility_rules_encrypted BLOB;")
        .map_err(|e| StorageError::Migration(format!("Add column: {}", e)))?;

    // 2. Read all contacts with plaintext visibility rules
    let mut stmt = conn
        .prepare("SELECT id, visibility_rules_json FROM contacts")
        .map_err(|e| StorageError::Migration(format!("Read contacts: {}", e)))?;

    let rows: Vec<(String, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| StorageError::Migration(format!("Query: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StorageError::Migration(format!("Collect: {}", e)))?;

    // 3. Encrypt each visibility rules JSON and write to new column
    for (id, json_opt) in &rows {
        if let Some(json) = json_opt {
            let encrypted = encrypt(key, json.as_bytes()).map_err(|e| {
                StorageError::Migration(format!("Encrypt visibility for {}: {}", id, e))
            })?;
            conn.execute(
                "UPDATE contacts SET visibility_rules_encrypted = ?1, visibility_rules_json = NULL WHERE id = ?2",
                rusqlite::params![encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
        }
    }

    Ok(())
}

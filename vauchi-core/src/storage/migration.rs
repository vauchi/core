// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Database Schema Migration Framework
//!
//! Provides versioned schema migrations with transactional safety.
//! Each migration has a version number, name, and either SQL or a Rust callback.
//! The runner tracks applied versions in a `schema_version` table and runs
//! pending migrations in order within a single transaction.
//!
//! ## Design Decision: Forward-Only Migrations (#17)
//!
//! This framework intentionally does not support down-migrations (rollback).
//! Rationale:
//! - Down-migrations are rarely tested and often broken in production
//! - Vauchi's data is encrypted at rest — reversing encryption migrations
//!   would require the original key, making rollback unsafe
//! - SQLite has no DROP COLUMN, making schema reversal impractical
//!
//! If a migration fails, the transaction is rolled back atomically.
//! If a deployed migration needs reversal, a new forward migration should
//! undo the changes explicitly.

use std::path::Path;

use rusqlite::Connection;

use crate::crypto::SymmetricKey;

use hmac::{Hmac, Mac};
use sha2::Sha256;

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
#[non_exhaustive]
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
    ///
    /// If `db_path` is provided and there are pending migrations, a backup is
    /// created at `<db_path>.pre-migration-v<current>.bak` before applying (#17).
    pub fn run(
        conn: &Connection,
        key: &SymmetricKey,
        migrations: &[Migration],
        db_path: Option<&Path>,
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

        // Reject databases created by a newer app version (downgrade prevention)
        let latest_migration = migrations.iter().map(|m| m.version).max().unwrap_or(0);
        if current_version > latest_migration {
            return Err(StorageError::Migration(format!(
                "Database schema v{} is newer than this app (max v{}). Please upgrade the app.",
                current_version, latest_migration
            )));
        }

        // Collect pending migrations
        let pending: Vec<&Migration> = migrations
            .iter()
            .filter(|m| m.version > current_version)
            .collect();

        if pending.is_empty() {
            return Ok(());
        }

        // Create pre-migration backup for file-based databases (#17).
        // Uses VACUUM INTO for a consistent snapshot. Failure is logged but
        // does not block migration — the transaction provides rollback safety.
        if let Some(path) = db_path {
            let backup_path =
                path.with_extension(format!("pre-migration-v{}.bak", current_version));
            // Backup failure is non-fatal — the transaction provides rollback safety.
            let _ = conn.execute(
                "VACUUM INTO ?1",
                rusqlite::params![backup_path.to_string_lossy().as_ref()],
            );
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
            let now = crate::clock::ambient_now_secs();

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

/// Idempotent ALTER TABLE ADD COLUMN: adds a column only if it doesn't already exist.
///
/// SQLite does not support `ADD COLUMN IF NOT EXISTS`. This helper queries
/// `PRAGMA table_info` to check before adding. Needed for crash-recovery
/// safety in v14/v15/v16/v18 encrypt-in-place migrations (Tracker #54).
fn add_column_if_not_exists(
    conn: &Connection,
    table: &str,
    column: &str,
    col_type: &str,
) -> Result<(), StorageError> {
    let exists: bool = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .map_err(|e| StorageError::Migration(format!("PRAGMA table_info({}): {}", table, e)))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| StorageError::Migration(format!("Query table_info({}): {}", table, e)))?
        .any(|name| name.as_deref() == Ok(column));

    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE {} ADD COLUMN {} {};",
            table, column, col_type
        ))
        .map_err(|e| {
            StorageError::Migration(format!(
                "ALTER TABLE {} ADD COLUMN {}: {}",
                table, column, e
            ))
        })?;
    }
    Ok(())
}

/// Encrypts data and verifies the ciphertext decrypts back to the original (#165a).
///
/// This prevents data loss during encrypt-in-place migrations: if the key is
/// wrong or encryption produces an unreadable blob, this function returns an
/// error before the plaintext column is cleared.
fn encrypt_and_verify(
    key: &SymmetricKey,
    plaintext: &[u8],
    context: &str,
) -> Result<Vec<u8>, StorageError> {
    use crate::crypto::{decrypt, encrypt};

    let encrypted = encrypt(key, plaintext)
        .map_err(|e| StorageError::Migration(format!("Encrypt {}: {}", context, e)))?;

    // Verify roundtrip: decrypt must produce the original plaintext
    let decrypted = decrypt(key, &encrypted)
        .map_err(|e| StorageError::Migration(format!("Roundtrip verify {}: {}", context, e)))?;

    if decrypted != plaintext {
        return Err(StorageError::Migration(format!(
            "Roundtrip mismatch for {} (encrypted {} bytes, decrypted {} bytes)",
            context,
            encrypted.len(),
            decrypted.len(),
        )));
    }

    Ok(encrypted)
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
            action: MigrationAction::Sql(""),
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
        Migration {
            version: 19,
            name: "app_password_schema",
            action: MigrationAction::Sql(MIGRATION_V19_APP_PASSWORD),
        },
        Migration {
            version: 20,
            name: "duress_settings",
            action: MigrationAction::Sql(MIGRATION_V20_DURESS_SETTINGS),
        },
        Migration {
            version: 21,
            name: "decoy_contacts",
            action: MigrationAction::Sql(MIGRATION_V21_DECOY_CONTACTS),
        },
        Migration {
            version: 22,
            name: "emergency_config",
            action: MigrationAction::Sql(MIGRATION_V22_EMERGENCY_CONFIG),
        },
        Migration {
            version: 23,
            name: "encrypt_label_names",
            action: MigrationAction::Callback(migrate_v23_encrypt_label_names),
        },
        Migration {
            version: 24,
            name: "per_contact_ratchet_keys",
            action: MigrationAction::Callback(migrate_v24_per_contact_ratchet_keys),
        },
        Migration {
            version: 25,
            name: "contact_delta_version_tracking",
            action: MigrationAction::Sql(MIGRATION_V25_DELTA_VERSION),
        },
        Migration {
            version: 26,
            name: "recovery_settings",
            action: MigrationAction::Sql(MIGRATION_V26_RECOVERY_SETTINGS),
        },
        Migration {
            version: 27,
            name: "trust_metric_fields",
            action: MigrationAction::Sql(MIGRATION_V27_TRUST_METRICS),
        },
        Migration {
            version: 28,
            name: "contact_limits_and_merge",
            action: MigrationAction::Sql(MIGRATION_V28_LIMITS_AND_MERGE),
        },
        Migration {
            version: 29,
            name: "onboarding_progress",
            action: MigrationAction::Sql(MIGRATION_V29_ONBOARDING_PROGRESS),
        },
        Migration {
            version: 30,
            name: "label_display_name_override",
            action: MigrationAction::Sql(MIGRATION_V30_LABEL_DISPLAY_NAME_OVERRIDE),
        },
        Migration {
            version: 31,
            name: "contact_relay_fields",
            action: MigrationAction::Sql(MIGRATION_V31_CONTACT_RELAY_FIELDS),
        },
        Migration {
            version: 32,
            name: "trust_and_notes",
            action: MigrationAction::Sql(MIGRATION_V32_TRUST_AND_NOTES),
        },
        Migration {
            version: 33,
            name: "trust_metrics_column",
            action: MigrationAction::Sql(MIGRATION_V33_TRUST_METRICS),
        },
        Migration {
            version: 34,
            name: "imported_contacts",
            action: MigrationAction::Sql(MIGRATION_V34_IMPORTED_CONTACTS),
        },
        Migration {
            version: 35,
            name: "local_groups",
            action: MigrationAction::Sql(MIGRATION_V35_LOCAL_GROUPS),
        },
        Migration {
            version: 36,
            name: "sent_delta_version_tracking",
            action: MigrationAction::Sql(MIGRATION_V36_SENT_DELTA_VERSION),
        },
        Migration {
            version: 37,
            name: "contact_delete_archive",
            action: MigrationAction::Sql(MIGRATION_V37_CONTACT_DELETE_ARCHIVE),
        },
        Migration {
            version: 38,
            name: "exchange_states",
            action: MigrationAction::Sql(MIGRATION_V38_EXCHANGE_STATES),
        },
        Migration {
            version: 39,
            name: "ohttp_key_cache",
            action: MigrationAction::Sql(MIGRATION_V39_OHTTP_KEY_CACHE),
        },
        Migration {
            version: 40,
            name: "reciprocity_confirmation",
            action: MigrationAction::Sql(MIGRATION_V40_RECIPROCITY),
        },
        Migration {
            version: 41,
            name: "activity_log",
            action: MigrationAction::Sql(MIGRATION_V41_ACTIVITY_LOG),
        },
        Migration {
            version: 42,
            name: "pin_cache",
            action: MigrationAction::Sql(MIGRATION_V42_PIN_CACHE),
        },
        Migration {
            version: 43,
            name: "contact_display",
            action: MigrationAction::Sql(MIGRATION_V43_CONTACT_DISPLAY),
        },
        Migration {
            version: 44,
            name: "backup_reminder",
            action: MigrationAction::Sql(MIGRATION_V44_BACKUP_REMINDER),
        },
        Migration {
            version: 45,
            name: "recovery_progress",
            action: MigrationAction::Sql(MIGRATION_V45_RECOVERY_PROGRESS),
        },
        Migration {
            version: 46,
            name: "app_preferences",
            action: MigrationAction::Sql(MIGRATION_V46_APP_PREFERENCES),
        },
    ]
}

// DEPRECATED: Tor support removed (2026-03-24). Column remains for
// DB schema compatibility. See 2026-03-24-ip-privacy-ohttp-strategy.
/// Migration v17: Add tor_config_encrypted column to ux_state table.
const MIGRATION_V17_TOR_CONFIG: &str = "
    ALTER TABLE ux_state ADD COLUMN tor_config_encrypted BLOB;
";

/// Migration v19: App password/PIN schema for duress PIN support.
///
/// Adds columns to the `identity` table for app-level password/PIN
/// authentication and duress PIN detection. All hash/salt columns are
/// encrypted BLOBs (application-level encryption with the storage master key).
/// The `duress_enabled` flag is an unencrypted INTEGER for quick checks.
const MIGRATION_V19_APP_PASSWORD: &str = "
    ALTER TABLE identity ADD COLUMN password_hash_encrypted BLOB;
    ALTER TABLE identity ADD COLUMN password_salt BLOB;
    ALTER TABLE identity ADD COLUMN duress_hash_encrypted BLOB;
    ALTER TABLE identity ADD COLUMN duress_salt BLOB;
    ALTER TABLE identity ADD COLUMN duress_enabled INTEGER DEFAULT 0;
";

/// Migration v20: Duress settings configuration table.
///
/// Singleton table (id = 1) storing encrypted duress configuration:
/// which contacts to alert, what message to send, and whether to
/// include device location in the alert. All sensitive fields are
/// encrypted BLOBs.
const MIGRATION_V20_DURESS_SETTINGS: &str = "
    CREATE TABLE IF NOT EXISTS duress_settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        alert_contact_ids_encrypted BLOB,
        alert_message_encrypted BLOB,
        include_location INTEGER DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v21: Decoy contacts table for duress mode.
///
/// Stores fake contacts displayed when the app is unlocked with the
/// duress PIN. Each decoy has a display name (plaintext for UI rendering)
/// and an encrypted card blob containing the full fake contact card.
const MIGRATION_V21_DECOY_CONTACTS: &str = "
    CREATE TABLE IF NOT EXISTS decoy_contacts (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        card_encrypted BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v22: Emergency broadcast configuration table.
///
/// Singleton table (id = 1) storing encrypted configuration for emergency
/// alerts: which contacts to alert, what message to send, and whether to
/// include device location. All sensitive fields are encrypted BLOBs.
const MIGRATION_V22_EMERGENCY_CONFIG: &str = "
    CREATE TABLE IF NOT EXISTS emergency_config (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        trusted_contact_ids_encrypted BLOB,
        message_encrypted BLOB,
        include_location INTEGER DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v23: Recovery settings persistence (#77).
///
/// Singleton table (id = 1) storing encrypted recovery thresholds.
/// `settings_encrypted` is a JSON blob encrypted with the storage key.
const MIGRATION_V26_RECOVERY_SETTINGS: &str = "
    CREATE TABLE IF NOT EXISTS recovery_settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        settings_encrypted BLOB NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v27: Add trust metric fields to contacts table.
///
/// - exchange_transport: TEXT (serde name: "Qr"/"Nfc"/"Ble"), default "Qr"
/// - has_recovered: INTEGER (boolean), default 0
/// - card_updated_at: INTEGER (unix timestamp), nullable
const MIGRATION_V27_TRUST_METRICS: &str = "
    ALTER TABLE contacts ADD COLUMN exchange_transport TEXT NOT NULL DEFAULT 'Qr';
    ALTER TABLE contacts ADD COLUMN has_recovered INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE contacts ADD COLUMN card_updated_at INTEGER;
";

/// Migration v28: Raise contact limit to 10,000 and add dismissed duplicates table.
///
/// - Updates the default contact limit from 500 to 10,000.
/// - Creates `dismissed_duplicates` table to track which duplicate suggestions
///   the user has dismissed, so they don't reappear.
const MIGRATION_V28_LIMITS_AND_MERGE: &str = "
    UPDATE contact_limits SET max_contacts = 10000 WHERE id = 1;

    CREATE TABLE IF NOT EXISTS dismissed_duplicates (
        id1 TEXT NOT NULL,
        id2 TEXT NOT NULL,
        dismissed_at INTEGER NOT NULL,
        PRIMARY KEY (id1, id2)
    );
";

/// Migration v29: Add onboarding progress column to ux_state table.
///
/// Stores encrypted onboarding progress (current step, completed steps,
/// timestamps) alongside other UX state.
const MIGRATION_V29_ONBOARDING_PROGRESS: &str = "
    ALTER TABLE ux_state ADD COLUMN onboarding_progress_encrypted BLOB;
";

/// Migration v30: Add display_name_override_encrypted column to visibility_labels.
///
/// Stores an optional encrypted display name override per label.
/// When set, contacts in this label see this name instead of the
/// user's default display name. NULL means no override (use default).
const MIGRATION_V30_LABEL_DISPLAY_NAME_OVERRIDE: &str = "
    ALTER TABLE visibility_labels ADD COLUMN display_name_override_encrypted BLOB;
";

/// Migration v31: Add relay fields to contacts and pending_updates tables.
///
/// Contacts: stores relay URL and Noise NK public key learned during
/// QR exchange. Both nullable — existing contacts load with None.
/// The relay URL is plaintext TEXT (public server address, not secret).
/// The Noise pubkey is a raw 32-byte BLOB.
///
/// Pending updates: stores the target relay URL for per-contact routing.
/// When set, the update should be dispatched to the contact's relay
/// instead of the home relay. NULL means use home relay.
const MIGRATION_V31_CONTACT_RELAY_FIELDS: &str = "
    ALTER TABLE contacts ADD COLUMN relay_url TEXT;
    ALTER TABLE contacts ADD COLUMN relay_noise_pubkey BLOB;
    ALTER TABLE pending_updates ADD COLUMN target_relay_url TEXT;
";

/// Migration v32: Trust indicator and private field notes.
///
/// Adds `proposal_trusted` flag to `contacts` for tracking whether the user
/// has explicitly trusted a contact's card proposal. Also creates the
/// `contact_field_notes` table for storing per-field encrypted private notes,
/// keyed by (contact_id, field_id). Notes are encrypted at the application
/// layer before storage.
const MIGRATION_V32_TRUST_AND_NOTES: &str = "
ALTER TABLE contacts ADD COLUMN proposal_trusted INTEGER DEFAULT 0;
CREATE TABLE IF NOT EXISTS contact_field_notes (
    contact_id TEXT NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL,
    note_encrypted BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (contact_id, field_id)
);
";

/// Migration v33: Trust metrics JSON column on contacts.
///
/// Stores the full `TrustMetrics` struct as JSON. NULL for legacy contacts
/// created before trust metrics were introduced. Enables auditable trust
/// derivation from exchange signals.
const MIGRATION_V33_TRUST_METRICS: &str = "
    ALTER TABLE contacts ADD COLUMN trust_metrics TEXT DEFAULT NULL;
";

/// Migration v34: Imported contacts support.
///
/// Adds columns to distinguish exchanged contacts from imported ones.
/// Existing contacts get `contact_kind = 'exchanged'` via DEFAULT.
/// `import_source`, `imported_at`, and `original_uid` are NULL for exchanged contacts.
const MIGRATION_V34_IMPORTED_CONTACTS: &str = "
    ALTER TABLE contacts ADD COLUMN contact_kind TEXT NOT NULL DEFAULT 'exchanged';
    ALTER TABLE contacts ADD COLUMN import_source TEXT;
    ALTER TABLE contacts ADD COLUMN imported_at INTEGER;
    ALTER TABLE contacts ADD COLUMN original_uid TEXT;
";

/// Migration v35: Local organization groups for imported contacts (HR-5).
///
/// Creates a table for user-defined local groups. Unlike visibility labels,
/// local groups have NO outbound sharing semantics — they're purely for the
/// user's local organization and are never transmitted to contacts.
/// Migration v36: Add `last_sent_delta_version` column to contacts.
///
/// Tracks the highest delta version SENT to each contact,
/// enabling proper downgrade detection on the receiver side.
/// Complements `last_delta_version` (which tracks received versions).
const MIGRATION_V36_SENT_DELTA_VERSION: &str = "
    ALTER TABLE contacts ADD COLUMN last_sent_delta_version INTEGER DEFAULT 0;
";

/// Migration v37: Add soft-delete and archive columns to contacts.
///
/// Supports the contact delete/archive feature:
/// - `deleted_at`: timestamp of soft-deletion (NULL = not deleted)
/// - `archived`: flag for archived contacts (0 = not archived)
/// - `archived_at`: timestamp of archival (NULL = not archived)
const MIGRATION_V37_CONTACT_DELETE_ARCHIVE: &str = "
    ALTER TABLE contacts ADD COLUMN deleted_at INTEGER;
    ALTER TABLE contacts ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE contacts ADD COLUMN archived_at INTEGER;
";

/// Migration v38: Persisted exchange state for crash recovery (Link mode).
///
/// Encrypted blob contains the full `PersistedExchangeState` JSON.
/// `exchange_id` is a 64-char hex string (TEXT) for readable lookups.
/// `expires_at` index enables efficient TTL sweep.
const MIGRATION_V38_EXCHANGE_STATES: &str = "
    CREATE TABLE IF NOT EXISTS exchange_states (
        exchange_id TEXT PRIMARY KEY,
        encrypted_blob BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_exchange_states_expires ON exchange_states(expires_at);
";

/// Migration v39: OHTTP key cache for relay-fetched keys.
///
/// Stores the most recently fetched OHTTP key per relay URL so that
/// callers can avoid redundant fetches across sessions. `fetched_at`
/// is a Unix-epoch seconds timestamp for TTL checking.
const MIGRATION_V39_OHTTP_KEY_CACHE: &str = "
    CREATE TABLE IF NOT EXISTS ohttp_key_cache (
        relay_url TEXT PRIMARY KEY,
        key_bytes BLOB NOT NULL,
        fetched_at INTEGER NOT NULL
    );
";

/// Migration v40: Reciprocity confirmation columns on contacts.
///
/// Tracks whether the other party also completed the exchange (orthogonal
/// to trust scoring per ADR-034). `confirmation_state` stores encrypted
/// confirmer state for relaunch recovery.
const MIGRATION_V40_RECIPROCITY: &str = "
    ALTER TABLE contacts ADD COLUMN reciprocity TEXT DEFAULT NULL;
    ALTER TABLE contacts ADD COLUMN confirmation_channel TEXT DEFAULT NULL;
    ALTER TABLE contacts ADD COLUMN confirmation_state BLOB DEFAULT NULL;
";

/// Migration v41: Activity log table for 7-day rolling window of user-visible events.
///
/// Stores card updates, exchanges, and emergency alerts. `event_key` is a
/// caller-generated deduplication key (INSERT OR IGNORE). `created_at` is
/// Unix seconds and is used for range queries and pruning.
const MIGRATION_V41_ACTIVITY_LOG: &str = "
    CREATE TABLE activity_log (
        event_key   TEXT PRIMARY KEY,
        category    TEXT NOT NULL,
        contact_id  TEXT,
        payload     TEXT NOT NULL,
        created_at  INTEGER NOT NULL
    );
";

/// Migration v42: Certificate pin cache for pin rotation.
///
/// Stores relay-served pin sets with TTL, enabling pin updates
/// without app releases. Mirrors the `ohttp_key_cache` pattern.
const MIGRATION_V42_PIN_CACHE: &str = "
    CREATE TABLE IF NOT EXISTS pin_cache (
        relay_url  TEXT PRIMARY KEY,
        pin_bytes  BLOB NOT NULL,
        fetched_at INTEGER NOT NULL
    );
";

/// Migration v43: Contact nickname, custom avatar, shared names/avatars, and display preferences.
///
/// Adds `contact_shared_names` and `contact_shared_avatars` tables for flat name/avatar sets,
/// and 4 new columns on `contacts` for local nickname, custom avatar, and
/// display preferences.
const MIGRATION_V43_CONTACT_DISPLAY: &str = "
    CREATE TABLE contact_shared_names (
        contact_id TEXT NOT NULL,
        name       TEXT NOT NULL,
        is_primary INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (contact_id, name),
        FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
    );

    CREATE TABLE contact_shared_avatars (
        contact_id       TEXT NOT NULL,
        avatar_hash      TEXT NOT NULL,
        avatar_encrypted BLOB NOT NULL,
        is_primary       INTEGER NOT NULL DEFAULT 0,
        updated_at       INTEGER NOT NULL,
        PRIMARY KEY (contact_id, avatar_hash),
        FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
    );

    ALTER TABLE contacts ADD COLUMN nickname_encrypted BLOB;
    ALTER TABLE contacts ADD COLUMN custom_avatar_encrypted BLOB;
    ALTER TABLE contacts ADD COLUMN display_name_preference TEXT NOT NULL DEFAULT '\"primary\"';
    ALTER TABLE contacts ADD COLUMN avatar_preference TEXT NOT NULL DEFAULT '\"primary\"';
";

/// Migration v44: Add backup_reminder column to ux_state.
const MIGRATION_V44_BACKUP_REMINDER: &str =
    "ALTER TABLE ux_state ADD COLUMN backup_reminder_encrypted BLOB;";

const MIGRATION_V35_LOCAL_GROUPS: &str = "
    CREATE TABLE IF NOT EXISTS local_groups (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        contact_ids_json TEXT NOT NULL DEFAULT '[]',
        created_at INTEGER NOT NULL
    );
";

/// Migration v44: In-progress recovery state persistence.
///
/// Singleton table (id = 1) storing one in-flight recovery session at a time.
/// `progress_encrypted` is the full `RecoveryProgress` struct serialized as JSON
/// and encrypted with the storage key. Only one recovery can be active at a time —
/// subsequent saves overwrite via INSERT OR REPLACE.
const MIGRATION_V45_RECOVERY_PROGRESS: &str = "
    CREATE TABLE IF NOT EXISTS recovery_progress (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        progress_encrypted BLOB NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Migration v46: App preferences singleton table.
///
/// Singleton table (id = 1) storing the user's theme + language picks.
/// `theme_id` and `language_code` are NULL when the user is following
/// the system default (the corresponding `follow_system_*` flag is the
/// authoritative signal — NULL alone could also mean "never set"). All
/// fields are unencrypted (preferences are not sensitive).
///
/// Wired from the Theme + Language pickers in the Settings screen via
/// `app_engine::intercept` and consumed by `SettingsConfig`. Replaces
/// the per-platform `SharedPreferences` / `UserDefaults` storage that
/// the bespoke `ThemeSettingsScreen` / `LanguageSettingsScreen`
/// composables wrote on Android (problem record
/// `2026-05-01-android-humble-ui-deep-retirement`, Phase 2a/A3a).
const MIGRATION_V46_APP_PREFERENCES: &str = "
    CREATE TABLE IF NOT EXISTS app_preferences (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        theme_id TEXT,
        language_code TEXT,
        follow_system_theme INTEGER NOT NULL DEFAULT 1,
        follow_system_language INTEGER NOT NULL DEFAULT 1,
        updated_at INTEGER NOT NULL
    );
";

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
/// - **Full deletion path**: Identity shredding (hard_shred / panic_shred)
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
///
/// ## Idempotency (Tracker #54)
///
/// Uses `add_column_if_not_exists` so that a crash after ALTER TABLE but before
/// COMMIT does not prevent the migration from re-running successfully.
///
/// ## No Pre-Migration Backup (Tracker #66)
///
/// This migration relies on SQLite transaction rollback for crash safety. No
/// file-level database backup (VACUUM INTO) is taken before encryption. A
/// pre-migration WAL checkpoint + file copy would provide defense-in-depth
/// against SQLite corruption scenarios.
///
/// ## ANR Risk on Mobile (Tracker #164)
///
/// All pending migrations run in a single `BEGIN EXCLUSIVE TRANSACTION`. For
/// databases with many rows, encrypt-in-place can take >5s on slow storage,
/// risking ANR on Android. Consider splitting into per-table transactions with
/// a progress callback for mobile clients.
fn migrate_v14_encrypt_high_priority(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    // Step 1: Add encrypted columns to each table (idempotent — Tracker #54)
    add_column_if_not_exists(conn, "own_card", "card_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "device_registry", "registry_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "device_sync_state", "state_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "visibility_labels", "contacts_json_encrypted", "BLOB")?;
    add_column_if_not_exists(
        conn,
        "visibility_labels",
        "visible_fields_json_encrypted",
        "BLOB",
    )?;

    // Step 2: Encrypt existing plaintext data in own_card
    {
        let result: Result<(String,), _> =
            conn.query_row("SELECT card_json FROM own_card WHERE id = 1", [], |row| {
                Ok((row.get(0)?,))
            });

        if let Ok((card_json,)) = result {
            let encrypted = encrypt_and_verify(key, card_json.as_bytes(), "own_card")?;
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
            let encrypted = encrypt_and_verify(key, registry_json.as_bytes(), "device_registry")?;
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
            let encrypted = encrypt_and_verify(key, state_json.as_bytes(), "device_sync_state")?;
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
            let contacts_enc = encrypt_and_verify(key, contacts_json.as_bytes(), "label_contacts")?;
            let fields_enc = encrypt_and_verify(key, fields_json.as_bytes(), "label_fields")?;
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
    // Step 1: Add encrypted columns to each table (idempotent — Tracker #54)
    add_column_if_not_exists(conn, "device_info", "device_info_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "version_vector", "vector_json_encrypted", "BLOB")?;
    add_column_if_not_exists(
        conn,
        "contact_sync_timestamps",
        "last_sync_at_encrypted",
        "BLOB",
    )?;
    add_column_if_not_exists(conn, "pending_updates", "payload_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "retry_entries", "payload_encrypted", "BLOB")?;
    add_column_if_not_exists(
        conn,
        "device_sync_checkpoints",
        "items_json_encrypted",
        "BLOB",
    )?;
    add_column_if_not_exists(conn, "recovery_responses", "response_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "deletion_state", "state_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "sync_checkpoints", "state_json_encrypted", "BLOB")?;

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
            let encrypted = encrypt_and_verify(key, &json_bytes, "device_info")?;
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
            let encrypted = encrypt_and_verify(key, vector_json.as_bytes(), "version_vector")?;
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
            let encrypted = encrypt_and_verify(key, &ts_bytes, "sync_timestamps")?;
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
            let encrypted = encrypt_and_verify(key, payload, "pending_updates")?;
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
            let encrypted = encrypt_and_verify(key, payload, "retry_entries")?;
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
            let encrypted = encrypt_and_verify(key, items_json.as_bytes(), "sync_checkpoints")?;
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
            let encrypted = encrypt_and_verify(key, response.as_bytes(), "recovery_responses")?;
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
            let encrypted = encrypt_and_verify(key, state_json.as_bytes(), "deletion_state")?;
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
            let encrypted = encrypt_and_verify(key, state_json.as_bytes(), "sync_checkpoint")?;
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
    // Step 1: Add encrypted columns (idempotent — Tracker #54)
    add_column_if_not_exists(conn, "field_validations", "field_value_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "field_validations", "signature_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "ux_state", "aha_tracker_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "ux_state", "demo_contact_json_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "audit_log", "details_encrypted", "BLOB")?;

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
            let fv_encrypted = encrypt_and_verify(key, field_value.as_bytes(), "field_value")?;
            let sig_encrypted = encrypt_and_verify(key, signature, "signature")?;
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
                    Some(encrypt_and_verify(key, json.as_bytes(), "aha_tracker")?)
                } else {
                    None
                }
            } else {
                None
            };

            let demo_encrypted = if let Some(ref json) = demo_json {
                if !json.is_empty() {
                    Some(encrypt_and_verify(key, json.as_bytes(), "demo_contact")?)
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
            let encrypted = encrypt_and_verify(key, details.as_bytes(), "audit_log")?;
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
    // 1. Add new encrypted column (idempotent — Tracker #54)
    add_column_if_not_exists(conn, "contacts", "visibility_rules_encrypted", "BLOB")?;

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
            let encrypted = encrypt_and_verify(key, json.as_bytes(), "visibility_rules")?;
            conn.execute(
                "UPDATE contacts SET visibility_rules_encrypted = ?1, visibility_rules_json = NULL WHERE id = ?2",
                rusqlite::params![encrypted, id],
            )
            .map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
        }
    }

    Ok(())
}

/// Migration v23: Encrypt label names (#128).
///
/// Adds `name_encrypted` BLOB and `name_hmac` BLOB columns to `visibility_labels`.
/// Encrypts existing plaintext names and computes HMAC for lookups.
/// After migration, the plaintext `name` column is blanked.
fn migrate_v23_encrypt_label_names(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::HKDF;

    // Add new columns (idempotent for crash recovery — F1 audit fix)
    add_column_if_not_exists(conn, "visibility_labels", "name_encrypted", "BLOB")?;
    add_column_if_not_exists(conn, "visibility_labels", "name_hmac", "BLOB")?;

    // Derive HMAC key for label name lookups
    let hmac_key_bytes = HKDF::derive_key(None, key.as_bytes(), b"Vauchi_Label_Name_HMAC_v1");
    type HmacSha256 = Hmac<Sha256>;

    // Encrypt existing plaintext names
    let mut stmt = conn
        .prepare("SELECT id, name FROM visibility_labels")
        .map_err(|e| StorageError::Migration(format!("Select labels: {}", e)))?;

    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| StorageError::Migration(format!("Query labels: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StorageError::Migration(format!("Collect labels: {}", e)))?;

    for (id, name) in &rows {
        let name_encrypted = encrypt_and_verify(key, name.as_bytes(), "label_name")?;
        let mut mac =
            HmacSha256::new_from_slice(&*hmac_key_bytes).expect("HMAC accepts any key length");
        mac.update(name.as_bytes());
        let name_hmac_value = mac.finalize().into_bytes();

        // Set plaintext name to the label id (satisfies UNIQUE constraint without leaking data)
        conn.execute(
            "UPDATE visibility_labels SET name_encrypted = ?1, name_hmac = ?2, name = ?3 WHERE id = ?3",
            rusqlite::params![name_encrypted, &name_hmac_value[..], id],
        )
        .map_err(|e| StorageError::Migration(format!("Update label {}: {}", id, e)))?;
    }

    Ok(())
}

/// Migration v24: Re-encrypt ratchet states with per-contact derived keys (#126).
///
/// Previously all ratchet states were encrypted with the shared storage master key.
/// This migration re-encrypts each row using an HKDF-derived per-contact key,
/// ensuring that compromising one contact's ratchet state does not expose the SMK.
/// Migration v25: Add `last_delta_version` column to contacts (#42).
///
/// Tracks the highest delta version received from each contact,
/// enabling rejection of stale/downgraded updates.
const MIGRATION_V25_DELTA_VERSION: &str = "
    ALTER TABLE contacts ADD COLUMN last_delta_version INTEGER DEFAULT 0;
";

fn migrate_v24_per_contact_ratchet_keys(
    conn: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    use crate::crypto::HKDF;

    // Load all ratchet rows
    let mut stmt = conn
        .prepare("SELECT contact_id, ratchet_state_encrypted FROM contact_ratchets")
        .map_err(|e| StorageError::Migration(format!("Select ratchets: {}", e)))?;

    let rows: Vec<(String, Vec<u8>)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| StorageError::Migration(format!("Query ratchets: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StorageError::Migration(format!("Collect ratchets: {}", e)))?;

    for (contact_id, encrypted) in &rows {
        // Decrypt with old shared key
        let plaintext = crate::crypto::decrypt(key, encrypted).map_err(|e| {
            StorageError::Migration(format!("Decrypt ratchet {}: {}", contact_id, e))
        })?;

        // Derive per-contact key
        let mut info = b"vauchi-ratchet-storage-v1:".to_vec();
        info.extend_from_slice(contact_id.as_bytes());
        let derived_bytes = HKDF::derive_key(None, key.as_bytes(), &info);
        let derived_key = SymmetricKey::from_bytes(*derived_bytes);

        // Re-encrypt with per-contact key + roundtrip verify (F2 audit fix)
        let re_encrypted = crate::crypto::encrypt(&derived_key, &plaintext).map_err(|e| {
            StorageError::Migration(format!("Re-encrypt ratchet {}: {}", contact_id, e))
        })?;
        let verify = crate::crypto::decrypt(&derived_key, &re_encrypted).map_err(|e| {
            StorageError::Migration(format!("Verify ratchet {}: {}", contact_id, e))
        })?;
        if verify != plaintext {
            return Err(StorageError::Migration(format!(
                "Ratchet roundtrip mismatch for {} ({} vs {} bytes)",
                contact_id,
                verify.len(),
                plaintext.len(),
            )));
        }

        conn.execute(
            "UPDATE contact_ratchets SET ratchet_state_encrypted = ?1 WHERE contact_id = ?2",
            rusqlite::params![re_encrypted, contact_id],
        )
        .map_err(|e| StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e)))?;
    }

    Ok(())
}

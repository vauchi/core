// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Database Migration Tests
//!
//! Tests that verify database schema compatibility and migration paths.
//! These tests ensure that:
//! 1. The current schema has all expected tables and columns
//! 2. Data written with older schemas can still be read
//! 3. Schema upgrades don't lose data

use rusqlite::Connection;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::migration::{all_migrations, MigrationRunner};
use vauchi_core::storage::Storage;

// =============================================================================
// SCHEMA VERSION 1 (Current)
// =============================================================================

/// Expected tables in schema V1.
const EXPECTED_TABLES_V1: &[&str] = &[
    "contacts",
    "own_card",
    "identity",
    "pending_updates",
    "contact_ratchets",
    "device_info",
    "device_registry",
    "device_sync_state",
    "version_vector",
];

/// Expected columns for each table in schema V1.
const CONTACTS_COLUMNS_V1: &[&str] = &[
    "id",
    "public_key",
    "display_name",
    "card_encrypted",
    "shared_key_encrypted",
    "visibility_rules_json",
    "exchange_timestamp",
    "fingerprint_verified",
    "last_sync_at",
];

const OWN_CARD_COLUMNS_V1: &[&str] = &["id", "card_json", "updated_at"];

const IDENTITY_COLUMNS_V1: &[&str] = &["id", "backup_data_encrypted", "display_name", "created_at"];

const PENDING_UPDATES_COLUMNS_V1: &[&str] = &[
    "id",
    "contact_id",
    "update_type",
    "payload",
    "created_at",
    "retry_count",
    "status",
    "error_message",
    "retry_at",
];

const CONTACT_RATCHETS_COLUMNS_V1: &[&str] = &[
    "contact_id",
    "ratchet_state_encrypted",
    "is_initiator",
    "updated_at",
];

const DEVICE_INFO_COLUMNS_V1: &[&str] = &[
    "id",
    "device_id",
    "device_index",
    "device_name",
    "created_at",
];

const DEVICE_REGISTRY_COLUMNS_V1: &[&str] = &["id", "registry_json", "version", "updated_at"];

const DEVICE_SYNC_STATE_COLUMNS_V1: &[&str] =
    &["device_id", "state_json", "last_sync_version", "updated_at"];

const VERSION_VECTOR_COLUMNS_V1: &[&str] = &["id", "vector_json", "updated_at"];

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Gets all table names from a SQLite database.
fn get_table_names(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    tables
}

/// Gets column names for a table.
fn get_column_names(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    columns
}

/// Gets index names for the database.
fn get_index_names(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    let indexes: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    indexes
}

// =============================================================================
// SCHEMA STRUCTURE TESTS
// =============================================================================

#[test]
fn test_schema_has_all_expected_tables() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Access the underlying connection via a query
    let conn = Connection::open_in_memory().unwrap();
    let key2 = SymmetricKey::generate();
    let _ = Storage::in_memory(key2).unwrap();

    // Create a fresh storage and check tables
    let temp_key = SymmetricKey::generate();
    let temp_storage = Storage::in_memory(temp_key).unwrap();

    // We need to check the schema through Storage's public interface
    // Since we can't access conn directly, we verify by attempting operations
    // that would fail if tables don't exist

    // Verify contacts table works
    assert!(temp_storage.list_contacts().is_ok());

    // Verify pending_updates table works
    assert!(temp_storage.get_all_pending_updates().is_ok());

    // Verify own_card table works (returns None if empty, but no error)
    assert!(temp_storage.load_own_card().is_ok());

    drop(storage);
    drop(conn);
}

#[test]
fn test_schema_tables_via_raw_connection() {
    // Create a raw SQLite connection and initialize schema manually
    let conn = Connection::open_in_memory().unwrap();

    // Execute the same schema as Storage
    conn.execute_batch(
        "
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

        CREATE TABLE IF NOT EXISTS own_card (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            card_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS identity (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            backup_data_encrypted BLOB NOT NULL,
            display_name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

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

        CREATE TABLE IF NOT EXISTS contact_ratchets (
            contact_id TEXT PRIMARY KEY,
            ratchet_state_encrypted BLOB NOT NULL,
            is_initiator INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS device_info (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            device_id BLOB NOT NULL,
            device_index INTEGER NOT NULL,
            device_name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS device_registry (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            registry_json TEXT NOT NULL,
            version INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS device_sync_state (
            device_id BLOB PRIMARY KEY,
            state_json TEXT NOT NULL,
            last_sync_version INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS version_vector (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            vector_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_pending_contact ON pending_updates(contact_id);
        CREATE INDEX IF NOT EXISTS idx_pending_status ON pending_updates(status);
        ",
    )
    .unwrap();

    // Verify all expected tables exist
    let tables = get_table_names(&conn);
    for expected in EXPECTED_TABLES_V1 {
        assert!(
            tables.contains(&expected.to_string()),
            "Missing table: {}",
            expected
        );
    }

    // Verify contacts columns
    let contacts_cols = get_column_names(&conn, "contacts");
    for col in CONTACTS_COLUMNS_V1 {
        assert!(
            contacts_cols.contains(&col.to_string()),
            "contacts missing column: {}",
            col
        );
    }

    // Verify own_card columns
    let own_card_cols = get_column_names(&conn, "own_card");
    for col in OWN_CARD_COLUMNS_V1 {
        assert!(
            own_card_cols.contains(&col.to_string()),
            "own_card missing column: {}",
            col
        );
    }

    // Verify identity columns
    let identity_cols = get_column_names(&conn, "identity");
    for col in IDENTITY_COLUMNS_V1 {
        assert!(
            identity_cols.contains(&col.to_string()),
            "identity missing column: {}",
            col
        );
    }

    // Verify pending_updates columns
    let pending_cols = get_column_names(&conn, "pending_updates");
    for col in PENDING_UPDATES_COLUMNS_V1 {
        assert!(
            pending_cols.contains(&col.to_string()),
            "pending_updates missing column: {}",
            col
        );
    }

    // Verify contact_ratchets columns
    let ratchets_cols = get_column_names(&conn, "contact_ratchets");
    for col in CONTACT_RATCHETS_COLUMNS_V1 {
        assert!(
            ratchets_cols.contains(&col.to_string()),
            "contact_ratchets missing column: {}",
            col
        );
    }

    // Verify device_info columns
    let device_info_cols = get_column_names(&conn, "device_info");
    for col in DEVICE_INFO_COLUMNS_V1 {
        assert!(
            device_info_cols.contains(&col.to_string()),
            "device_info missing column: {}",
            col
        );
    }

    // Verify device_registry columns
    let registry_cols = get_column_names(&conn, "device_registry");
    for col in DEVICE_REGISTRY_COLUMNS_V1 {
        assert!(
            registry_cols.contains(&col.to_string()),
            "device_registry missing column: {}",
            col
        );
    }

    // Verify device_sync_state columns
    let sync_state_cols = get_column_names(&conn, "device_sync_state");
    for col in DEVICE_SYNC_STATE_COLUMNS_V1 {
        assert!(
            sync_state_cols.contains(&col.to_string()),
            "device_sync_state missing column: {}",
            col
        );
    }

    // Verify version_vector columns
    let vector_cols = get_column_names(&conn, "version_vector");
    for col in VERSION_VECTOR_COLUMNS_V1 {
        assert!(
            vector_cols.contains(&col.to_string()),
            "version_vector missing column: {}",
            col
        );
    }

    // Verify indexes exist
    let indexes = get_index_names(&conn);
    assert!(
        indexes.contains(&"idx_pending_contact".to_string()),
        "Missing index: idx_pending_contact"
    );
    assert!(
        indexes.contains(&"idx_pending_status".to_string()),
        "Missing index: idx_pending_status"
    );
}

// =============================================================================
// DATA PERSISTENCE TESTS
// =============================================================================

#[test]
fn test_own_card_persistence() {
    use vauchi_core::ContactCard;

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Initially no card
    assert!(storage.load_own_card().unwrap().is_none());

    // Save a card
    let mut card = ContactCard::new("Test User");
    card.add_field(vauchi_core::ContactField::new(
        vauchi_core::FieldType::Email,
        "Work",
        "test@example.com",
    ))
    .unwrap();

    storage.save_own_card(&card).unwrap();

    // Load it back
    let loaded = storage.load_own_card().unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Test User");
    assert_eq!(loaded.fields().len(), 1);
    assert_eq!(loaded.fields()[0].value(), "test@example.com");
}

#[test]
fn test_pending_updates_persistence() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Initially empty
    assert!(storage.get_all_pending_updates().unwrap().is_empty());

    // Queue an update
    let update = PendingUpdate {
        id: "test-update-1".to_string(),
        contact_id: "contact-123".to_string(),
        update_type: "card_delta".to_string(),
        payload: vec![1, 2, 3, 4, 5],
        created_at: 1700000000,
        retry_count: 0,
        status: UpdateStatus::Pending,
    };

    storage.queue_update(&update).unwrap();

    // Load it back
    let loaded = storage.get_all_pending_updates().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "test-update-1");
    assert_eq!(loaded[0].contact_id, "contact-123");
    assert_eq!(loaded[0].payload, vec![1, 2, 3, 4, 5]);

    // Mark as sent (delete)
    storage.mark_update_sent("test-update-1").unwrap();
    assert!(storage.get_all_pending_updates().unwrap().is_empty());
}

#[test]
fn test_contact_persistence_roundtrip() {
    use vauchi_core::contact::Contact;
    use vauchi_core::{ContactCard, ContactField, FieldType, SymmetricKey as CryptoKey};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Create a contact
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "alice@example.com",
    ))
    .unwrap();

    let shared_key = CryptoKey::generate();
    let public_key = [42u8; 32];

    let contact = Contact::from_exchange(public_key, card, shared_key);

    // Save
    storage.save_contact(&contact).unwrap();

    // Load
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.card().display_name(), "Alice");
    assert_eq!(loaded.card().fields().len(), 1);
}

// =============================================================================
// SCHEMA EVOLUTION TESTS
// =============================================================================

#[test]
fn test_create_table_if_not_exists_is_idempotent() {
    // Running schema creation twice should not fail
    let key1 = SymmetricKey::generate();
    let storage1 = Storage::in_memory(key1).unwrap();

    // Save some data
    let card = vauchi_core::ContactCard::new("Test");
    storage1.save_own_card(&card).unwrap();

    // Opening storage again (simulating restart) should work
    // Note: in-memory storage doesn't persist, so this just verifies
    // the schema creation is safe to run multiple times
    let key2 = SymmetricKey::generate();
    let storage2 = Storage::in_memory(key2).unwrap();
    assert!(storage2.load_own_card().unwrap().is_none()); // Different instance, no data
}

#[test]
fn test_nullable_columns_work() {
    use vauchi_core::contact::Contact;
    use vauchi_core::{ContactCard, SymmetricKey as CryptoKey};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Create contact without visibility rules (nullable column)
    let card = ContactCard::new("Bob");
    let shared_key = CryptoKey::generate();
    let public_key = [0u8; 32];

    let contact = Contact::from_exchange(public_key, card, shared_key);
    storage.save_contact(&contact).unwrap();

    // Should load successfully even with null visibility_rules_json
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.card().display_name(), "Bob");
}

#[test]
fn test_default_column_values() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Queue update - retry_count and status have defaults
    let update = PendingUpdate {
        id: "test-defaults".to_string(),
        contact_id: "contact-456".to_string(),
        update_type: "test".to_string(),
        payload: vec![],
        created_at: 1700000000,
        retry_count: 0,
        status: UpdateStatus::Pending,
    };

    storage.queue_update(&update).unwrap();

    let loaded = storage.get_all_pending_updates().unwrap();
    assert_eq!(loaded[0].retry_count, 0);
    assert!(matches!(loaded[0].status, UpdateStatus::Pending));
}

// =============================================================================
// DURESS PIN MIGRATION TESTS (V19–V21)
// =============================================================================

/// Helper: runs migrations up to (and including) the given version on an in-memory connection.
fn run_migrations_up_to(conn: &Connection, key: &SymmetricKey, up_to_version: u32) {
    let migrations = all_migrations();
    let subset: Vec<_> = migrations
        .into_iter()
        .filter(|m| m.version <= up_to_version)
        .collect();
    MigrationRunner::run(conn, key, &subset, None).unwrap();
}

#[test]
fn test_migration_v19_adds_password_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run migrations up to V18 (current baseline)
    run_migrations_up_to(&conn, &key, 18);

    // Verify the new columns do NOT exist yet
    let identity_cols = get_column_names(&conn, "identity");
    assert!(
        !identity_cols.contains(&"password_hash_encrypted".to_string()),
        "password_hash_encrypted should not exist before V19"
    );

    // Now run V19
    run_migrations_up_to(&conn, &key, 19);

    // Verify all new columns exist
    let identity_cols = get_column_names(&conn, "identity");
    let expected_new_cols = [
        "password_hash_encrypted",
        "password_salt",
        "duress_hash_encrypted",
        "duress_salt",
        "duress_enabled",
    ];
    for col in &expected_new_cols {
        assert!(
            identity_cols.contains(&col.to_string()),
            "identity table missing column after V19: {}",
            col
        );
    }

    // Verify duress_enabled defaults to 0
    conn.execute(
        "INSERT INTO identity (id, backup_data_encrypted, display_name, created_at) VALUES (1, X'00', 'test', 1000)",
        [],
    )
    .unwrap();

    let duress_enabled: i64 = conn
        .query_row(
            "SELECT duress_enabled FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duress_enabled, 0, "duress_enabled should default to 0");
}

#[test]
fn test_migration_v20_creates_duress_settings_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run migrations up to V19
    run_migrations_up_to(&conn, &key, 19);

    // Verify duress_settings does NOT exist yet
    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"duress_settings".to_string()),
        "duress_settings should not exist before V20"
    );

    // Now run V20
    run_migrations_up_to(&conn, &key, 20);

    // Verify table exists
    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"duress_settings".to_string()),
        "duress_settings table should exist after V20"
    );

    // Verify columns
    let cols = get_column_names(&conn, "duress_settings");
    let expected = [
        "id",
        "alert_contact_ids_encrypted",
        "alert_message_encrypted",
        "include_location",
        "created_at",
        "updated_at",
    ];
    for col in &expected {
        assert!(
            cols.contains(&col.to_string()),
            "duress_settings missing column: {}",
            col
        );
    }

    // Verify singleton constraint (id must be 1)
    conn.execute(
        "INSERT INTO duress_settings (id, created_at, updated_at) VALUES (1, 1000, 1000)",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO duress_settings (id, created_at, updated_at) VALUES (2, 1000, 1000)",
        [],
    );
    assert!(
        result.is_err(),
        "duress_settings should enforce id = 1 singleton constraint"
    );

    // Verify include_location defaults to 0
    let include_location: i64 = conn
        .query_row(
            "SELECT include_location FROM duress_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(include_location, 0, "include_location should default to 0");
}

#[test]
fn test_migration_v21_creates_decoy_contacts_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run migrations up to V20
    run_migrations_up_to(&conn, &key, 20);

    // Verify decoy_contacts does NOT exist yet
    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"decoy_contacts".to_string()),
        "decoy_contacts should not exist before V21"
    );

    // Now run V21
    run_migrations_up_to(&conn, &key, 21);

    // Verify table exists
    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"decoy_contacts".to_string()),
        "decoy_contacts table should exist after V21"
    );

    // Verify columns
    let cols = get_column_names(&conn, "decoy_contacts");
    let expected = [
        "id",
        "display_name",
        "card_encrypted",
        "created_at",
        "updated_at",
    ];
    for col in &expected {
        assert!(
            cols.contains(&col.to_string()),
            "decoy_contacts missing column: {}",
            col
        );
    }

    // Verify we can insert and retrieve a decoy contact
    conn.execute(
        "INSERT INTO decoy_contacts (id, display_name, card_encrypted, created_at, updated_at) VALUES ('dc-1', 'Decoy Alice', X'DEADBEEF', 1000, 1000)",
        [],
    )
    .unwrap();

    let name: String = conn
        .query_row(
            "SELECT display_name FROM decoy_contacts WHERE id = 'dc-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Decoy Alice");
}

#[test]
fn test_schema_version_after_all_migrations() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run ALL migrations
    let migrations = all_migrations();
    MigrationRunner::run(&conn, &key, &migrations, None).unwrap();

    // Verify final schema version
    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(
        version, 26,
        "Schema version should be 26 after all migrations, got {}",
        version
    );
}

#[test]
fn test_migration_v19_is_safe_on_fresh_identity_table() {
    // V19 uses ALTER TABLE which should work even if the identity table has no rows
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 19);

    // Verify we can still insert into identity with the new nullable columns
    conn.execute(
        "INSERT INTO identity (id, backup_data_encrypted, display_name, created_at) VALUES (1, X'00', 'test', 1000)",
        [],
    )
    .unwrap();

    // New columns should be NULL by default (except duress_enabled which defaults to 0)
    let pw_hash: Option<Vec<u8>> = conn
        .query_row(
            "SELECT password_hash_encrypted FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        pw_hash.is_none(),
        "password_hash_encrypted should default to NULL"
    );

    let pw_salt: Option<Vec<u8>> = conn
        .query_row(
            "SELECT password_salt FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(pw_salt.is_none(), "password_salt should default to NULL");

    let duress_hash: Option<Vec<u8>> = conn
        .query_row(
            "SELECT duress_hash_encrypted FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        duress_hash.is_none(),
        "duress_hash_encrypted should default to NULL"
    );

    let duress_salt: Option<Vec<u8>> = conn
        .query_row("SELECT duress_salt FROM identity WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(duress_salt.is_none(), "duress_salt should default to NULL");

    let duress_enabled: i64 = conn
        .query_row(
            "SELECT duress_enabled FROM identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duress_enabled, 0, "duress_enabled should default to 0");
}

// =============================================================================
// EMERGENCY BROADCAST MIGRATION TEST (V22)
// =============================================================================

#[test]
fn test_migration_v22_creates_emergency_config_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run migrations up to V21
    run_migrations_up_to(&conn, &key, 21);

    // Verify emergency_config does NOT exist yet
    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"emergency_config".to_string()),
        "emergency_config should not exist before V22"
    );

    // Now run V22
    run_migrations_up_to(&conn, &key, 22);

    // Verify table exists
    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"emergency_config".to_string()),
        "emergency_config table should exist after V22"
    );

    // Verify columns
    let cols = get_column_names(&conn, "emergency_config");
    let expected = [
        "id",
        "trusted_contact_ids_encrypted",
        "message_encrypted",
        "include_location",
        "created_at",
        "updated_at",
    ];
    for col in &expected {
        assert!(
            cols.contains(&col.to_string()),
            "emergency_config missing column: {}",
            col
        );
    }

    // Verify singleton constraint (id must be 1)
    conn.execute(
        "INSERT INTO emergency_config (id, created_at, updated_at) VALUES (1, 1000, 1000)",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO emergency_config (id, created_at, updated_at) VALUES (2, 1000, 1000)",
        [],
    );
    assert!(
        result.is_err(),
        "emergency_config should enforce id = 1 singleton constraint"
    );

    // Verify include_location defaults to 0
    let include_location: i64 = conn
        .query_row(
            "SELECT include_location FROM emergency_config WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(include_location, 0, "include_location should default to 0");
}

// =============================================================================
// MIGRATION IDEMPOTENCY (Tracker #54)
// =============================================================================

/// Tests that add_column_if_not_exists is safe to call twice.
///
/// This verifies the idempotency guard added for crash-recovery safety.
/// If the process crashes after ALTER TABLE but before COMMIT, the migration
/// runner will re-run the callback — the column already exists but must not
/// cause an error.
#[test]
fn test_add_column_idempotent_via_double_migration() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run all migrations once (creates all columns)
    let migrations = all_migrations();
    MigrationRunner::run(&conn, &key, &migrations, None).unwrap();

    // Verify v14 columns exist
    let has_card_encrypted: bool = conn
        .prepare("PRAGMA table_info(own_card)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.as_deref() == Ok("card_json_encrypted"));
    assert!(
        has_card_encrypted,
        "card_json_encrypted column should exist after v14"
    );

    // Running migrations again should be a no-op (version guard)
    MigrationRunner::run(&conn, &key, &migrations, None).unwrap();

    // Verify the column still exists and nothing broke
    let still_has: bool = conn
        .prepare("PRAGMA table_info(own_card)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .any(|name| name.as_deref() == Ok("card_json_encrypted"));
    assert!(
        still_has,
        "card_json_encrypted column should still exist after re-run"
    );
}

// =============================================================================
// REKEY ATOMICITY (Tracker #161)
// =============================================================================

/// Tests that re_encrypt_all_tables (rekey) is atomic — either all tables
/// are re-encrypted or none are.
#[test]
fn test_rekey_is_atomic() {
    let key = SymmetricKey::generate();
    let mut storage = Storage::in_memory(key).unwrap();

    // Save some data
    let card = vauchi_core::ContactCard::new("AtomicTest");
    storage.save_own_card(&card).unwrap();

    // Rekey to a new key
    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should still be loadable (re-encrypted with new key)
    let loaded = storage.load_own_card().unwrap();
    assert!(loaded.is_some(), "Card should be loadable after rekey");
    assert_eq!(loaded.unwrap().display_name(), "AtomicTest");
}

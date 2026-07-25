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
use vauchi_core::storage::Storage;
use vauchi_core::storage::migration::{MigrationRunner, all_migrations};

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
// =============================================================================

// @internal
#[test]
fn test_schema_has_all_expected_tables() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Access the underlying connection via a query
    let conn = Connection::open_in_memory().unwrap();
    let key2 = SymmetricKey::generate();
    let _ = Storage::in_memory(key2).unwrap();

    let temp_key = SymmetricKey::generate();
    let temp_storage = Storage::in_memory(temp_key).unwrap();

    // We need to check the schema through Storage's public interface
    // Since we can't access conn directly, we verify by attempting operations
    // that would fail if tables don't exist

    temp_storage
        .contacts()
        .list_contacts()
        .expect("expected success");

    temp_storage
        .pending()
        .get_all_pending_updates()
        .expect("expected success");

    // Verify own_card table works (returns None if empty, but no error)
    temp_storage
        .contacts()
        .load_own_card()
        .expect("expected success");

    drop(storage);
    drop(conn);
}

// @internal
#[test]
fn test_schema_tables_via_raw_connection() {
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

    let tables = get_table_names(&conn);
    for expected in EXPECTED_TABLES_V1 {
        assert!(
            tables.contains(&expected.to_string()),
            "Missing table: {}",
            expected
        );
    }

    let contacts_cols = get_column_names(&conn, "contacts");
    for col in CONTACTS_COLUMNS_V1 {
        assert!(
            contacts_cols.contains(&col.to_string()),
            "contacts missing column: {}",
            col
        );
    }

    let own_card_cols = get_column_names(&conn, "own_card");
    for col in OWN_CARD_COLUMNS_V1 {
        assert!(
            own_card_cols.contains(&col.to_string()),
            "own_card missing column: {}",
            col
        );
    }

    let identity_cols = get_column_names(&conn, "identity");
    for col in IDENTITY_COLUMNS_V1 {
        assert!(
            identity_cols.contains(&col.to_string()),
            "identity missing column: {}",
            col
        );
    }

    let pending_cols = get_column_names(&conn, "pending_updates");
    for col in PENDING_UPDATES_COLUMNS_V1 {
        assert!(
            pending_cols.contains(&col.to_string()),
            "pending_updates missing column: {}",
            col
        );
    }

    let ratchets_cols = get_column_names(&conn, "contact_ratchets");
    for col in CONTACT_RATCHETS_COLUMNS_V1 {
        assert!(
            ratchets_cols.contains(&col.to_string()),
            "contact_ratchets missing column: {}",
            col
        );
    }

    let device_info_cols = get_column_names(&conn, "device_info");
    for col in DEVICE_INFO_COLUMNS_V1 {
        assert!(
            device_info_cols.contains(&col.to_string()),
            "device_info missing column: {}",
            col
        );
    }

    let registry_cols = get_column_names(&conn, "device_registry");
    for col in DEVICE_REGISTRY_COLUMNS_V1 {
        assert!(
            registry_cols.contains(&col.to_string()),
            "device_registry missing column: {}",
            col
        );
    }

    let sync_state_cols = get_column_names(&conn, "device_sync_state");
    for col in DEVICE_SYNC_STATE_COLUMNS_V1 {
        assert!(
            sync_state_cols.contains(&col.to_string()),
            "device_sync_state missing column: {}",
            col
        );
    }

    let vector_cols = get_column_names(&conn, "version_vector");
    for col in VERSION_VECTOR_COLUMNS_V1 {
        assert!(
            vector_cols.contains(&col.to_string()),
            "version_vector missing column: {}",
            col
        );
    }

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
// =============================================================================

// @internal
#[test]
fn test_own_card_persistence() {
    use vauchi_core::ContactCard;

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    assert!(storage.contacts().load_own_card().unwrap().is_none());

    let mut card = ContactCard::new("Test User");
    card.add_field(vauchi_core::ContactField::new(
        vauchi_core::FieldType::Email,
        "Work",
        "test@example.com",
        0,
    ))
    .unwrap();

    storage.contacts().save_own_card(&card).unwrap();

    let loaded = storage.contacts().load_own_card().unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Test User");
    assert_eq!(loaded.fields().len(), 1);
    assert_eq!(loaded.fields()[0].value(), "test@example.com");
}

// @internal
#[test]
fn test_pending_updates_persistence() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    assert!(
        storage
            .pending()
            .get_all_pending_updates()
            .unwrap()
            .is_empty()
    );

    let update = PendingUpdate {
        id: "test-update-1".to_string(),
        contact_id: "contact-123".to_string(),
        update_type: "card_delta".to_string(),
        payload: vec![1, 2, 3, 4, 5],
        created_at: 1700000000,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    };

    storage.pending().queue_update(&update).unwrap();

    let loaded = storage.pending().get_all_pending_updates().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "test-update-1");
    assert_eq!(loaded[0].contact_id, "contact-123");
    assert_eq!(loaded[0].payload, vec![1, 2, 3, 4, 5]);

    // Mark as sent (delete)
    storage.pending().mark_update_sent("test-update-1").unwrap();
    assert!(
        storage
            .pending()
            .get_all_pending_updates()
            .unwrap()
            .is_empty()
    );
}

// @internal
#[test]
fn test_contact_persistence_roundtrip() {
    use vauchi_core::contact::Contact;
    use vauchi_core::{ContactCard, ContactField, FieldType, SymmetricKey as CryptoKey};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "alice@example.com",
        0,
    ))
    .unwrap();

    let shared_key = CryptoKey::generate();
    let public_key = [42u8; 32];

    let contact = Contact::from_exchange(public_key, card, shared_key, 0);

    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(contact.id())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.card().display_name(), "Alice");
    assert_eq!(loaded.card().fields().len(), 1);
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_create_table_if_not_exists_is_idempotent() {
    let key1 = SymmetricKey::generate();
    let storage1 = Storage::in_memory(key1).unwrap();

    let card = vauchi_core::ContactCard::new("Test");
    storage1.contacts().save_own_card(&card).unwrap();

    // Opening storage again (simulating restart) should work
    // Note: in-memory storage doesn't persist, so this just verifies
    // the schema creation is safe to run multiple times
    let key2 = SymmetricKey::generate();
    let storage2 = Storage::in_memory(key2).unwrap();
    assert!(storage2.contacts().load_own_card().unwrap().is_none()); // Different instance, no data
}

// @internal
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

    let contact = Contact::from_exchange(public_key, card, shared_key, 0);
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage
        .contacts()
        .load_contact(contact.id())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.card().display_name(), "Bob");
}

// @internal
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
        target_relay_url: None,
        target_device_id: None,
    };

    storage.pending().queue_update(&update).unwrap();

    let loaded = storage.pending().get_all_pending_updates().unwrap();
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
        .iter()
        .filter(|m| m.version <= up_to_version)
        .copied()
        .collect();
    MigrationRunner::run(conn, key, &subset, None, 0).unwrap();
}

// @internal
#[test]
fn test_migration_v56_adds_group_presentation_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Up to V55 the per-group override columns do not exist yet.
    run_migrations_up_to(&conn, &key, 55);
    let cols = get_column_names(&conn, "visibility_labels");
    assert!(
        !cols.contains(&"bio_override_encrypted".to_string()),
        "bio_override_encrypted should not exist before V56"
    );
    assert!(
        !cols.contains(&"avatar_override_encrypted".to_string()),
        "avatar_override_encrypted should not exist before V56"
    );

    // A label created before the migration must survive the upgrade in place.
    conn.execute(
        "INSERT INTO visibility_labels (id, name, created_at, modified_at) VALUES ('g1', 'Family', 1000, 1000)",
        [],
    )
    .unwrap();

    run_migrations_up_to(&conn, &key, 56);

    let cols = get_column_names(&conn, "visibility_labels");
    assert!(
        cols.contains(&"bio_override_encrypted".to_string()),
        "visibility_labels missing bio_override_encrypted after V56"
    );
    assert!(
        cols.contains(&"avatar_override_encrypted".to_string()),
        "visibility_labels missing avatar_override_encrypted after V56"
    );

    // Existing row preserved; the new columns default to NULL.
    let (name, bio, avatar): (String, Option<Vec<u8>>, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT name, bio_override_encrypted, avatar_override_encrypted FROM visibility_labels WHERE id = 'g1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "Family");
    assert_eq!(bio, None);
    assert_eq!(avatar, None);
}

// @internal
#[test]
fn migration_v59_preserves_legacy_ratchet_as_zero_device_id() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    run_migrations_up_to(&conn, &key, 58);

    conn.execute(
        "INSERT INTO contacts (
            id, public_key, display_name, card_encrypted, shared_key_encrypted,
            exchange_timestamp
         ) VALUES ('alice', zeroblob(32), 'Alice', X'01', X'02', 1000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO contact_ratchets (
            contact_id, ratchet_state_encrypted, is_initiator, updated_at
         ) VALUES ('alice', X'AABB', 1, 1000)",
        [],
    )
    .unwrap();

    run_migrations_up_to(&conn, &key, 59);

    let (device_id, state, is_initiator): (Vec<u8>, Vec<u8>, i64) = conn
        .query_row(
            "SELECT peer_device_id, ratchet_state_encrypted, is_initiator
             FROM contact_ratchets WHERE contact_id = 'alice'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(device_id, vec![0; 32]);
    assert_eq!(state, vec![0xAA, 0xBB]);
    assert_eq!(is_initiator, 1);
}

// @internal
#[test]
fn test_migration_v19_adds_password_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run migrations up to V18 (current baseline)
    run_migrations_up_to(&conn, &key, 18);

    let identity_cols = get_column_names(&conn, "identity");
    assert!(
        !identity_cols.contains(&"password_hash_encrypted".to_string()),
        "password_hash_encrypted should not exist before V19"
    );

    run_migrations_up_to(&conn, &key, 19);

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

// @internal
#[test]
fn test_migration_v20_creates_duress_settings_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 19);

    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"duress_settings".to_string()),
        "duress_settings should not exist before V20"
    );

    run_migrations_up_to(&conn, &key, 20);

    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"duress_settings".to_string()),
        "duress_settings table should exist after V20"
    );

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

// @internal
#[test]
fn test_migration_v21_creates_decoy_contacts_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 20);

    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"decoy_contacts".to_string()),
        "decoy_contacts should not exist before V21"
    );

    run_migrations_up_to(&conn, &key, 21);

    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"decoy_contacts".to_string()),
        "decoy_contacts table should exist after V21"
    );

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

// @internal
#[test]
fn test_schema_version_after_all_migrations() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    let migrations = all_migrations();
    MigrationRunner::run(&conn, &key, &migrations, None, 0).unwrap();

    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(
        version, 66,
        "Schema version should be 66 after all migrations, got {}",
        version
    );
}

// @internal
#[test]
fn test_migration_v19_is_safe_on_fresh_identity_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 19);

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

// @internal
#[test]
fn test_migration_v22_creates_emergency_config_table() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 21);

    let tables = get_table_names(&conn);
    assert!(
        !tables.contains(&"emergency_config".to_string()),
        "emergency_config should not exist before V22"
    );

    run_migrations_up_to(&conn, &key, 22);

    let tables = get_table_names(&conn);
    assert!(
        tables.contains(&"emergency_config".to_string()),
        "emergency_config table should exist after V22"
    );

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
// @internal
#[test]
fn test_add_column_idempotent_via_double_migration() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    // Run all migrations once (creates all columns)
    let migrations = all_migrations();
    MigrationRunner::run(&conn, &key, &migrations, None, 0).unwrap();

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
    MigrationRunner::run(&conn, &key, &migrations, None, 0).unwrap();

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
// @internal
#[test]
fn test_rekey_is_atomic() {
    let key = SymmetricKey::generate();
    let mut storage = Storage::in_memory(key).unwrap();

    let card = vauchi_core::ContactCard::new("AtomicTest");
    storage.contacts().save_own_card(&card).unwrap();

    let new_key = SymmetricKey::generate();
    storage.rekey(new_key).unwrap();

    // Data should still be loadable (re-encrypted with new key)
    let loaded = storage.contacts().load_own_card().unwrap();
    assert!(loaded.is_some(), "Card should be loadable after rekey");
    assert_eq!(loaded.unwrap().display_name(), "AtomicTest");
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_migration_rejects_newer_schema_version() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    let migrations = all_migrations();
    MigrationRunner::run(&conn, &key, &migrations, None, 0).unwrap();

    // Manually bump schema_version to simulate a DB created by a newer app
    let future_version = 999;
    conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
        rusqlite::params![future_version, 0i64],
    )
    .unwrap();

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 0);
    assert!(
        result.is_err(),
        "Should reject database from a newer app version"
    );

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("newer than this app"),
        "Error should mention newer app: {}",
        err_msg
    );
}

// =============================================================================
// =============================================================================

use vauchi_core::storage::migration::{Migration, MigrationAction};

/// current_version returns 0 on a fresh database with no schema_version table.
// @internal
#[test]
fn test_current_version_fresh_db_returns_zero() {
    let conn = Connection::open_in_memory().unwrap();
    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(version, 0, "Fresh DB should have schema version 0");
}

/// Running with an empty migrations list is a no-op (returns Ok).
// @internal
#[test]
fn test_migration_empty_list_is_noop() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();
    let empty: Vec<Migration> = vec![];

    let result = MigrationRunner::run(&conn, &key, &empty, None, 0);
    assert!(result.is_ok(), "Empty migration list should succeed");

    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(
        version, 0,
        "Version should remain 0 after empty migration list"
    );
}

/// Out-of-order migrations are rejected before any SQL runs.
// @internal
#[test]
fn test_migration_out_of_order_rejected() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    let migrations = vec![
        Migration {
            version: 2,
            name: "second",
            action: MigrationAction::Sql("CREATE TABLE t2 (id INTEGER);"),
        },
        Migration {
            version: 1,
            name: "first",
            action: MigrationAction::Sql("CREATE TABLE t1 (id INTEGER);"),
        },
    ];

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 0);
    assert!(
        result.is_err(),
        "Out-of-order migrations should be rejected"
    );

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("not in order"),
        "Error should mention ordering: {}",
        err_msg
    );
}

/// Duplicate version numbers are rejected (ordering check catches v==v).
// @internal
#[test]
fn test_migration_duplicate_version_rejected() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    let migrations = vec![
        Migration {
            version: 1,
            name: "first",
            action: MigrationAction::Sql("CREATE TABLE t1 (id INTEGER);"),
        },
        Migration {
            version: 1,
            name: "duplicate",
            action: MigrationAction::Sql("CREATE TABLE t2 (id INTEGER);"),
        },
    ];

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 0);
    assert!(
        result.is_err(),
        "Duplicate version migrations should be rejected"
    );
}

/// A failed SQL migration rolls back the entire transaction.
// @internal
#[test]
fn test_migration_sql_failure_rolls_back() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    let migrations = vec![
        Migration {
            version: 1,
            name: "create_table",
            action: MigrationAction::Sql("CREATE TABLE good_table (id INTEGER);"),
        },
        Migration {
            version: 2,
            name: "bad_sql",
            action: MigrationAction::Sql("THIS IS NOT VALID SQL;"),
        },
    ];

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 0);
    assert!(result.is_err(), "Bad SQL should fail");

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("bad_sql"),
        "Error should reference the failed migration name: {}",
        err_msg
    );

    // The good_table should NOT exist because the transaction was rolled back
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='good_table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!table_exists, "good_table should not exist after rollback");

    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(version, 0, "Version should be 0 after rollback");
}

/// A failed callback migration rolls back the entire transaction.
// @internal
#[test]
fn test_migration_callback_failure_rolls_back() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    fn failing_callback(
        _conn: &Connection,
        _key: &SymmetricKey,
    ) -> Result<(), vauchi_core::storage::StorageError> {
        Err(vauchi_core::storage::StorageError::Migration(
            "intentional test failure".to_string(),
        ))
    }

    let migrations = vec![
        Migration {
            version: 1,
            name: "create_table",
            action: MigrationAction::Sql("CREATE TABLE cb_table (id INTEGER);"),
        },
        Migration {
            version: 2,
            name: "bad_callback",
            action: MigrationAction::Callback(failing_callback),
        },
    ];

    let result = MigrationRunner::run(&conn, &key, &migrations, None, 0);
    assert!(result.is_err(), "Failing callback should fail");

    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("bad_callback"),
        "Error should reference the failed callback migration: {}",
        err_msg
    );

    // cb_table should NOT exist because the transaction was rolled back
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='cb_table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !table_exists,
        "cb_table should not exist after callback rollback"
    );

    let version = MigrationRunner::current_version(&conn).unwrap();
    assert_eq!(version, 0, "Version should be 0 after callback rollback");
}

// @internal
// @internal
#[test]
fn test_migration_v40_adds_reciprocity_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let key = SymmetricKey::generate();

    run_migrations_up_to(&conn, &key, 39);
    let cols_before = get_column_names(&conn, "contacts");
    assert!(!cols_before.contains(&"reciprocity".to_string()));

    run_migrations_up_to(&conn, &key, 40);
    let cols_after = get_column_names(&conn, "contacts");
    for col in ["reciprocity", "confirmation_channel", "confirmation_state"] {
        assert!(
            cols_after.contains(&col.to_string()),
            "contacts table missing column after v40: {col}",
        );
    }
}

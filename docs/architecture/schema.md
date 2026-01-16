# Database Schema

SQLite database with application-level encryption. Sensitive fields are encrypted with the user's storage key (derived from master seed).

**Implementation**: `webbook-core/src/storage/mod.rs`

## Tables

### contacts

Stores exchanged contacts and their encrypted cards.

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT PK | Contact ID (derived from public key hash) |
| public_key | BLOB | Contact's Ed25519 public key (32 bytes) |
| display_name | TEXT | Cached display name for listing |
| card_encrypted | BLOB | AES-256-GCM encrypted ContactCard JSON |
| shared_key_encrypted | BLOB | Encrypted per-contact symmetric key |
| visibility_rules_json | TEXT | JSON visibility rules (nullable) |
| exchange_timestamp | INTEGER | Unix timestamp of exchange |
| fingerprint_verified | INTEGER | 1 if manually verified, 0 otherwise |
| last_sync_at | INTEGER | Last successful sync timestamp (nullable) |

**Indexes**: None beyond PK (list is sorted in application)

### own_card

User's own contact card (single row).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Always 1 (singleton) |
| card_json | TEXT | ContactCard as JSON (not encrypted, local only) |
| updated_at | INTEGER | Last modification timestamp |

### identity

User's identity backup data (single row).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Always 1 (singleton) |
| backup_data_encrypted | BLOB | Encrypted master seed + metadata |
| display_name | TEXT | User's display name |
| created_at | INTEGER | Identity creation timestamp |

### contact_ratchets

Double Ratchet state for each contact (forward secrecy).

| Column | Type | Description |
|--------|------|-------------|
| contact_id | TEXT PK | FK to contacts.id |
| ratchet_state_encrypted | BLOB | Encrypted serialized DoubleRatchetState |
| is_initiator | INTEGER | 1 if we initiated exchange |
| updated_at | INTEGER | Last ratchet operation timestamp |

**Note**: Deleted when contact is deleted (CASCADE via application).

### pending_updates

Outbound message queue for offline sync.

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT PK | Update UUID |
| contact_id | TEXT | Target contact (no FK for pre-exchange queuing) |
| update_type | TEXT | "card_update", "visibility_change", etc. |
| payload | BLOB | Encrypted update payload |
| created_at | INTEGER | Queue timestamp |
| retry_count | INTEGER | Number of send attempts |
| status | TEXT | "pending", "sending", "failed" |
| error_message | TEXT | Last error (nullable) |
| retry_at | INTEGER | Next retry timestamp (nullable) |

**Indexes**:
- `idx_pending_contact(contact_id)` - Find updates for contact
- `idx_pending_status(status)` - Find by status

### device_info

Current device information (single row).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Always 1 (singleton) |
| device_id | BLOB | This device's ID (32 bytes, derived from seed + index) |
| device_index | INTEGER | Device index (0 = primary, 1+ = linked) |
| device_name | TEXT | User-friendly device name |
| created_at | INTEGER | Device registration timestamp |

### device_registry

Registry of all linked devices (single row, signed).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Always 1 (singleton) |
| registry_json | TEXT | Serialized DeviceRegistry (includes signature) |
| version | INTEGER | Monotonic registry version |
| updated_at | INTEGER | Last modification timestamp |

**Contents of registry_json**:
```json
{
  "devices": [
    {
      "device_id": "base64...",
      "device_index": 0,
      "name": "iPhone",
      "public_key": "base64...",
      "added_at": 1704067200,
      "revoked": false
    }
  ],
  "version": 2,
  "signature": "base64..."
}
```

### device_sync_state

Per-device sync state for inter-device synchronization.

| Column | Type | Description |
|--------|------|-------------|
| device_id | BLOB PK | Target device ID (32 bytes) |
| state_json | TEXT | Serialized InterDeviceSyncState |
| last_sync_version | INTEGER | Last synced version number |
| updated_at | INTEGER | Last state update timestamp |

**Contents of state_json**:
```json
{
  "device_id": "base64...",
  "pending_items": [
    {"type": "CardUpdated", "field_label": "email", "new_value": "...", "timestamp": 1234}
  ],
  "last_sync_version": 5
}
```

### version_vector

Local version vector for causality tracking (single row).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Always 1 (singleton) |
| vector_json | TEXT | Serialized VersionVector |
| updated_at | INTEGER | Last update timestamp |

**Contents of vector_json**:
```json
{
  "versions": {
    "device_id_base64_1": 5,
    "device_id_base64_2": 3
  }
}
```

## Encryption Details

| Data | Encryption | Key Source |
|------|------------|------------|
| contact.card_encrypted | AES-256-GCM | Storage key (from master seed) |
| contact.shared_key_encrypted | AES-256-GCM | Storage key |
| identity.backup_data_encrypted | AES-256-GCM | Storage key |
| ratchet_state_encrypted | AES-256-GCM | Storage key |
| pending_updates.payload | Double Ratchet | Per-contact ratchet |

**Storage key derivation**: `HKDF(master_seed, "WebBook_Storage")`

## Schema Version

Current version: **1** (initial)

### Migration Strategy

When schema changes are needed:

1. Add migration function in `storage/mod.rs`
2. Check `PRAGMA user_version` on open
3. Run migrations sequentially
4. Update `user_version`

```rust
// Example migration pattern
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    conn.execute("ALTER TABLE contacts ADD COLUMN new_field TEXT", [])?;
    conn.pragma_update(None, "user_version", 2)?;
    Ok(())
}
```

## Entity Relationship

```
┌─────────────┐       ┌──────────────────┐
│  identity   │       │  device_registry │
│  (1 row)    │       │  (1 row)         │
└─────────────┘       └──────────────────┘
                              │
                              │ contains
                              ▼
┌─────────────┐       ┌──────────────────┐
│ device_info │       │ device_sync_state│
│ (1 row)     │       │ (per device)     │
└─────────────┘       └──────────────────┘

┌─────────────┐       ┌──────────────────┐
│  own_card   │       │  version_vector  │
│  (1 row)    │       │  (1 row)         │
└─────────────┘       └──────────────────┘

┌─────────────┐  1:1  ┌──────────────────┐
│  contacts   │──────▶│ contact_ratchets │
└─────────────┘       └──────────────────┘
       │
       │ 1:N
       ▼
┌─────────────────┐
│ pending_updates │
└─────────────────┘
```

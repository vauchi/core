# Code Structure

## Module Organization

```
vauchi-mobile/
├── src/
│   └── lib.rs           # All mobile bindings in one file
├── uniffi-bindgen.rs    # UniFFI CLI binary
└── Cargo.toml           # Crate configuration
```

## Components

### `lib.rs` - Mobile Bindings

Single file containing all UniFFI-exposed types and implementations.

| Section | Purpose |
|---------|---------|
| Error Types | `MobileError` enum for mobile-friendly errors |
| Data Types | Records for fields, cards, contacts |
| `VauchiMobile` | Main interface object with all operations |
| Tests | Unit tests for mobile API |

### Key Types

| Type | UniFFI Attr | Description |
|------|-------------|-------------|
| `MobileError` | `uniffi::Error` | Error enum with string messages |
| `MobileFieldType` | `uniffi::Enum` | Contact field types |
| `MobileContactField` | `uniffi::Record` | Single field data |
| `MobileContactCard` | `uniffi::Record` | Card with fields |
| `MobileContact` | `uniffi::Record` | Contact with metadata |
| `MobileExchangeData` | `uniffi::Record` | QR data for sharing |
| `MobileExchangeResult` | `uniffi::Record` | Exchange result |
| `MobileSyncStatus` | `uniffi::Enum` | Sync state |
| `MobileSocialNetwork` | `uniffi::Record` | Social network info |
| `VauchiMobile` | `uniffi::Object` | Main interface |

### `VauchiMobile` Methods

| Category | Methods |
|----------|---------|
| Identity | `has_identity`, `create_identity`, `get_public_id`, `get_display_name` |
| Card | `get_own_card`, `add_field`, `update_field`, `remove_field`, `set_display_name` |
| Contacts | `list_contacts`, `get_contact`, `search_contacts`, `remove_contact`, `verify_contact` |
| Visibility | `hide_field_from_contact`, `show_field_to_contact`, `is_field_visible_to_contact` |
| Exchange | `generate_exchange_qr`, `complete_exchange` |
| Sync | `sync`, `get_sync_status`, `pending_update_count` |
| Backup | `export_backup`, `import_backup` |
| Social | `list_social_networks`, `search_social_networks`, `get_profile_url` |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `vauchi-core` | Core library (without network features) |
| `uniffi` | Cross-language bindings |
| `serde` | Serialization for internal use |
| `thiserror` | Error derivation |
| `parking_lot` | Sync primitives |
| `base64` | Backup encoding |
| `hex` | Public ID encoding |

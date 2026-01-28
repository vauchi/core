<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Code Structure

## Module Organization

```
vauchi-core/
├── src/
│   ├── lib.rs              # Crate root, public exports
│   ├── api/                # High-level API
│   ├── contact/            # Contact + visibility rules
│   ├── contact_card/       # Contact card data structure
│   ├── crypto/             # Cryptographic primitives
│   ├── exchange/           # Key exchange protocols
│   ├── identity/           # User identity management
│   ├── network/            # Transport abstraction
│   ├── storage/            # SQLite persistence
│   └── sync/               # Synchronization protocol
└── tests/
    └── integration_tests.rs
```

## Modules

### `api/` - High-Level API
The main entry point for applications using Vauchi.

| File | Purpose |
|------|---------|
| `vauchi.rs` | Main `Vauchi` orchestrator and builder |
| `config.rs` | Configuration types (`VauchiConfig`, `RelayConfig`) |
| `contact_manager.rs` | Contact CRUD operations |
| `sync_controller.rs` | Sync + network coordination |
| `events.rs` | Event system (`VauchiEvent`, handlers) |
| `error.rs` | Unified `VauchiError` type |

### `crypto/` - Cryptographic Primitives
Low-level cryptographic operations using the `ring` crate.

| File | Purpose |
|------|---------|
| `signing.rs` | Ed25519 signatures (`SigningKey`, `VerifyingKey`) |
| `encryption.rs` | AES-256-GCM encryption (`SymmetricKey`) |
| `key_exchange.rs` | X25519 key agreement (`ExchangeKey`) |
| `kdf.rs` | HKDF key derivation |
| `chain.rs` | Symmetric ratchet chain |
| `ratchet.rs` | Double Ratchet implementation |

### `identity/` - Identity Management
User identity derived from a master seed.

| File | Purpose |
|------|---------|
| `mod.rs` | `Identity` struct with signing and exchange keys |
| `backup.rs` | Seed backup and recovery |

### `contact_card/` - Contact Card Structure
The core data model for contact information.

| File | Purpose |
|------|---------|
| `mod.rs` | `ContactCard` with versioning |
| `field.rs` | `ContactField` and `FieldType` enum |
| `validation.rs` | Field value validation (email, phone, URL) |

### `exchange/` - Contact Exchange
Protocols for exchanging keys with new contacts.

| File | Purpose |
|------|---------|
| `x3dh.rs` | X3DH key agreement (`X3DHKeyPair`, `X3DHBundle`) |
| `qr.rs` | QR code generation and parsing |
| `ble.rs` | Bluetooth LE exchange (placeholder) |
| `proximity.rs` | Proximity detection helpers |
| `session.rs` | Exchange session state machine |
| `error.rs` | Exchange-specific errors |

### `contact/` - Contact Management
Contact storage and visibility controls.

| File | Purpose |
|------|---------|
| `mod.rs` | `Contact` struct with card and crypto state |
| `visibility.rs` | Per-field visibility rules |

### `storage/` - Persistence
SQLite-based encrypted storage.

| File | Purpose |
|------|---------|
| `mod.rs` | `Storage` with encrypted blob storage |

### `network/` - Transport Layer
Network abstraction for relay communication.

| File | Purpose |
|------|---------|
| `transport.rs` | `Transport` trait definition |
| `message.rs` | Wire message types (`MessageEnvelope`) |
| `protocol.rs` | Serialization and framing |
| `connection.rs` | Connection state machine |
| `relay_client.rs` | Relay client with acknowledgment tracking |
| `mock.rs` | Mock transport for testing |
| `error.rs` | Network-specific errors |

### `sync/` - Synchronization Protocol
Delta-based sync for efficient updates.

| File | Purpose |
|------|---------|
| `mod.rs` | `SyncManager` with queue management |
| `delta.rs` | `CardDelta` computation and application |
| `state.rs` | `SyncState` enum (Synced, Pending, Failed) |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `ring` | Audited cryptographic operations |
| `rusqlite` | SQLite database |
| `serde` | Serialization |
| `uuid` | Unique identifiers |
| `thiserror` | Error type derivation |
| `base64` | Binary encoding |
| `qrcode` | QR code generation |

## Test Coverage

- 267 tests across all modules
- Unit tests in each module
- Integration tests in `tests/integration_tests.rs`

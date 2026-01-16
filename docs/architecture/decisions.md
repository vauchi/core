# Architecture Decision Records

Key technical decisions and their rationale. New decisions append to this file.

---

## ADR-001: Rust for Core Library

**Status**: Decided
**Date**: 2024-01

**Context**: Need a language for the shared core library that compiles to mobile (iOS/Android), desktop, and WebAssembly.

**Decision**: Rust

**Rationale**:
- Memory safety without garbage collection
- Compiles to native (mobile via UniFFI, desktop) and WASM
- Strong ecosystem for cryptography (`ring` crate)
- Predictable performance for crypto operations

**Alternatives Considered**:
- C/C++: Memory safety concerns, harder FFI
- Go: GC pauses, larger binary size, poor WASM support
- Kotlin Multiplatform: iOS support immature at time of decision

---

## ADR-002: ring Crate for Cryptography

**Status**: Decided
**Date**: 2024-01

**Context**: Need production-ready cryptographic primitives.

**Decision**: Use `ring` crate exclusively for all crypto.

**Rationale**:
- Audited, production-ready (used by rustls, Cloudflare)
- No custom crypto implementations
- Supports Ed25519, X25519, AES-GCM, PBKDF2, HKDF
- Constant-time operations where needed

**Consequences**:
- Cannot use XChaCha20-Poly1305 directly (ring doesn't have it)
- Using AES-256-GCM instead for symmetric encryption
- Tests must use real crypto, no mocking

---

## ADR-003: SQLite for Local Storage

**Status**: Decided
**Date**: 2024-01

**Context**: Need local persistence that works on all platforms.

**Decision**: SQLite with application-level encryption.

**Rationale**:
- Available on all platforms (mobile, desktop, WASM via sql.js)
- Single-file database, easy backup
- Application-level encryption allows fine-grained control
- Proven reliability

**Alternatives Considered**:
- SQLCipher: Adds native dependency complexity
- LevelDB/RocksDB: No SQL, harder queries
- Custom file format: Reinventing the wheel

---

## ADR-004: WebSocket Relay for MVP Sync

**Status**: Decided
**Date**: 2024-03

**Context**: Need to sync updates between contacts when both online or one offline.

**Decision**: WebSocket relay server for MVP; libp2p DHT planned for future.

**Rationale**:
- Simpler NAT traversal than P2P
- Faster to implement and deploy
- Store-and-forward handles offline contacts
- Can migrate to DHT later without protocol changes

**Consequences**:
- Relay is a potential single point of failure (mitigated by federation plan)
- Relay sees encrypted blobs but no plaintext
- Users can self-host relay

**Future**: Migrate to libp2p with DHT discovery for fully decentralized sync.

---

## ADR-005: JSON for Internal Serialization

**Status**: Decided
**Date**: 2024-06

**Context**: Need serialization format for storage and sync payloads.

**Decision**: JSON (via serde_json)

**Rationale**:
- Human-readable for debugging
- No schema compilation step
- Sufficient performance for our data sizes (<64KB cards)
- Universal support across platforms

**Alternatives Considered**:
- Protocol Buffers: Schema compilation, overkill for simple structures
- MessagePack: Binary, harder to debug, marginal size benefit
- CBOR: Less ecosystem support

**Note**: Wire protocol uses encrypted binary blobs; JSON is internal only.

---

## ADR-006: X3DH + Double Ratchet for Key Exchange

**Status**: Decided
**Date**: 2024-02

**Context**: Need secure key exchange with forward secrecy for contact communication.

**Decision**: X3DH for initial exchange, Double Ratchet for ongoing messages.

**Rationale**:
- Industry standard (Signal Protocol)
- Forward secrecy: Past messages safe if keys compromised
- Future secrecy: Compromised keys heal after ratchet step
- Well-understood security properties

**Implementation**:
- X3DH: `webbook-core/src/exchange/x3dh.rs`
- Double Ratchet: `webbook-core/src/crypto/ratchet.rs`

---

## ADR-007: HKDF for Device Key Derivation

**Status**: Decided
**Date**: 2026-01

**Context**: Multi-device sync requires per-device keys derived from master seed.

**Decision**: HKDF with domain separation for all key derivation.

**Rationale**:
- Deterministic: Same seed + index = same device keys
- Domain separation prevents key reuse across contexts
- Standard (RFC 5869), implemented in `ring`

**Derivation Paths**:
```
Master Seed
├── "WebBook_Identity" → Ed25519 signing keypair
├── "WebBook_Exchange_Seed" → X25519 exchange keypair
└── "WebBook_Device_{index}" → Per-device keys
```

---

## ADR-008: Version Vectors for Sync Conflict Resolution

**Status**: Decided
**Date**: 2026-01

**Context**: Multi-device sync needs to detect and resolve conflicts.

**Decision**: Version vectors with last-write-wins resolution.

**Rationale**:
- Tracks causality across devices
- Detects concurrent modifications
- Simple LWW resolution appropriate for contact cards
- No need for complex CRDT merge logic

**Implementation**: `webbook-core/src/sync/device_sync.rs:VersionVector`

---

## ADR-009: QR + Proximity for Contact Exchange

**Status**: Decided
**Date**: 2024-02

**Context**: Need secure in-person contact exchange.

**Decision**: QR code as primary method; BLE/NFC as future options.

**Rationale**:
- QR works on all devices with camera
- No special hardware required
- Proximity requirement prevents remote harvesting
- BLE/NFC can be added later for convenience

**Security**:
- QR contains X25519 public key + identity signature
- Proximity verified by exchange completion timing
- Audio verification planned for additional security

---

## ADR-010: UniFFI for Mobile Bindings

**Status**: Decided
**Date**: 2024-08

**Context**: Need to expose Rust core to iOS (Swift) and Android (Kotlin).

**Decision**: UniFFI for generating mobile bindings.

**Rationale**:
- Single interface definition, generates Swift + Kotlin
- Handles memory management across FFI boundary
- Maintained by Mozilla, production-ready
- Cleaner than manual JNI/C bindings

**Implementation**: `webbook-mobile/` crate with UDL definitions.

---

## ADR-011: Event-Driven Architecture

**Status**: Decided
**Date**: 2026-01

**Context**: Mobile and desktop applications need to react to asynchronous events (contact updates, sync state changes, connection status) without polling.

**Decision**: Implement an event system with typed events, handler traits, and a central dispatcher.

**Rationale**:
- Decouples core logic from UI layers
- Enables multiple listeners (logging, UI updates, analytics)
- Type-safe event handling via `WebBookEvent` enum
- Thread-safe with `Send + Sync` requirements

**Implementation**:
- `WebBookEvent`: Enum of all possible events
- `EventHandler`: Trait for event consumers
- `EventDispatcher`: Broadcasts events to registered handlers
- Location: `webbook-core/src/api/events.rs`

---

## ADR-012: Visibility Rule Enforcement

**Status**: Decided
**Date**: 2026-01

**Context**: Users need fine-grained control over which contacts can see which fields on their contact card.

**Decision**: Per-field visibility rules with three levels: Everyone, specific Contacts, or Nobody.

**Rationale**:
- Simple mental model for users
- Flexible enough for privacy needs
- Serializable for sync between devices
- Efficient filtering at query time

**Implementation**:
- `FieldVisibility`: Enum (Everyone | Contacts(HashSet) | Nobody)
- `VisibilityRules`: HashMap<field_id, FieldVisibility>
- `can_see(field, contact)`: Fast visibility check
- Location: `webbook-core/src/contact/visibility.rs`

---

## ADR-013: Unified Error Types

**Status**: Decided
**Date**: 2026-01

**Context**: Multiple crates have their own error types. API layer needs consistent error handling for consumers.

**Decision**: Single `WebBookError` enum that wraps all domain-specific errors via `#[from]`.

**Rationale**:
- Single error type for public API
- Automatic conversion from internal errors via `thiserror`
- Preserves error context through the chain
- Consistent error messages for UI display

**Implementation**:
- `WebBookError`: Top-level enum with variants for each domain
- `WebBookResult<T>`: Type alias for `Result<T, WebBookError>`
- Variants: Validation, Exchange, Storage, Sync, Network, etc.
- Location: `webbook-core/src/api/error.rs`

---

## ADR-014: Pending Update Queue

**Status**: Decided
**Date**: 2026-01

**Context**: Devices may be offline when changes occur. Updates must be queued and retried reliably.

**Decision**: SQLite-backed pending update queue with status tracking and retry support.

**Rationale**:
- Survives app restarts (persisted)
- Supports retry with exponential backoff
- Tracks per-update status (Pending, Sending, Failed)
- Ordered by creation time

**Implementation**:
- `PendingUpdate`: Struct with id, contact_id, payload, retry_count, status
- `UpdateStatus`: Enum (Pending | Sending | Failed)
- Storage methods: `queue_update`, `get_pending_updates`, `mark_update_sent`
- Location: `webbook-core/src/storage/mod.rs`

---

## ADR-015: Application-Level Encryption

**Status**: Decided
**Date**: 2026-01

**Context**: SQLite stores sensitive data (contact cards, shared keys, ratchet states). Need encryption at rest.

**Decision**: Application-level encryption for sensitive fields using AES-256-GCM.

**Rationale**:
- Works on all platforms without SQLCipher dependency
- Fine-grained control over what gets encrypted
- Encryption key derived from user's master key
- Non-sensitive metadata (IDs, timestamps) remain queryable

**Encrypted Fields**:
- `card_encrypted`: Contact cards (JSON)
- `shared_key_encrypted`: Per-contact symmetric keys
- `ratchet_state_encrypted`: Double Ratchet state
- `backup_data_encrypted`: Identity backup

**Implementation**: `webbook-core/src/storage/mod.rs`

---

## ADR-016: Social Network Registry

**Status**: Decided
**Date**: 2026-01

**Context**: Contact cards include social media profiles. Need standardized network identifiers and profile URL generation.

**Decision**: Embedded JSON registry of 35+ social networks with URL templates.

**Rationale**:
- Compile-time embedding via `include_str!`
- No network requests needed for URL generation
- Handles username normalization (stripping @, etc.)
- Extensible via merge for custom networks

**Implementation**:
- `SocialNetwork`: Network definition with URL template
- `SocialNetworkRegistry`: HashMap with search, URL generation
- `networks.json`: Embedded data file
- Location: `webbook-core/src/social/registry.rs`

---

## ADR-017: Sync Controller Orchestration

**Status**: Decided
**Date**: 2026-01

**Context**: Synchronization involves multiple components: relay client, sync manager, ratchet states, events. Need central coordination.

**Decision**: `SyncController` as the orchestration layer integrating all sync concerns.

**Rationale**:
- Single entry point for sync operations
- Manages ratchet lifecycle per contact
- Coordinates relay connection and pending updates
- Emits events for UI feedback
- Handles timeout and retry logic

**Responsibilities**:
- Connection lifecycle (connect/disconnect)
- Ratchet registration per contact
- Sync cycle execution (send pending, process acks)
- Device sync integration

**Implementation**: `webbook-core/src/api/sync_controller.rs`

---

## Template for New Decisions

```markdown
## ADR-XXX: Title

**Status**: Proposed | Decided | Superseded
**Date**: YYYY-MM

**Context**: What problem are we solving?

**Decision**: What did we decide?

**Rationale**: Why this approach?

**Alternatives Considered**: What else did we evaluate?

**Consequences**: What are the trade-offs?
```

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

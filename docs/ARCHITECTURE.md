# WebBook Architecture Plan

## Overview

WebBook is a privacy-focused, decentralized contact card exchange application that allows users to share and update contact information with people they meet in the physical world.

## Core Principles

1. **Privacy First**: All data is end-to-end encrypted
2. **Decentralized**: Minimize reliance on central servers
3. **Physical Proximity Required**: Contact exchange only happens in-person
4. **User Control**: Users decide what information each contact can see
5. **Real-time Updates**: Changes propagate to authorized contacts automatically

---

## System Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         CLIENT DEVICES                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐            │
│  │   iOS    │  │ Android  │  │ Desktop  │  │   Web    │            │
│  │   App    │  │   App    │  │   App    │  │   App    │            │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘            │
│       │             │             │             │                    │
│       └─────────────┼─────────────┼─────────────┘                   │
│                     │             │                                  │
│              ┌──────┴─────────────┴──────┐                          │
│              │    Shared Core Library    │                          │
│              │    (Rust/WebAssembly)     │                          │
│              └──────────────┬────────────┘                          │
└─────────────────────────────┼───────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    COMMUNICATION LAYER                               │
│  ┌─────────────────────┐    ┌─────────────────────────────────┐     │
│  │  Proximity Exchange │    │      P2P Sync Network           │     │
│  │  (BLE/NFC/QR+Sound) │    │  (libp2p/DHT for discovery)     │     │
│  └─────────────────────┘    └─────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  OPTIONAL RELAY INFRASTRUCTURE                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │              Volunteer-Run Relay Nodes                       │    │
│  │         (Only encrypted blobs, no plaintext data)            │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Component Design

### 1. Identity & Cryptography Layer

#### User Identity
- Each user generates a **Ed25519 keypair** on first launch
- Public key serves as the user's unique identifier
- Private key never leaves the device (stored in secure enclave when available)

#### Key Derivation
```
Master Seed (256-bit)
    │
    ├── Identity Keypair (Ed25519) - for signing
    │
    ├── Exchange Keypair (X25519) - for key exchange
    │
    └── Per-Contact Symmetric Keys (derived via X3DH)
```

#### Encryption Scheme
- **Contact Card Encryption**: XChaCha20-Poly1305 with per-contact keys
- **Key Exchange**: X3DH (Extended Triple Diffie-Hellman) for initial exchange
- **Forward Secrecy**: Double Ratchet algorithm for update propagation

### 2. Contact Card Data Model

```typescript
interface ContactCard {
  id: string;                    // Public key fingerprint
  displayName: string;           // User's chosen display name
  avatar?: EncryptedBlob;        // Optional small avatar
  fields: ContactField[];        // Contact information fields
  lastModified: timestamp;       // For sync ordering
  signature: Signature;          // Self-signed for authenticity
}

interface ContactField {
  id: string;                    // Unique field identifier
  type: ContactFieldType;        // phone | email | social | address | custom
  label: string;                 // User-defined label (e.g., "Work Phone")
  value: string;                 // The actual contact info
  visibility: VisibilityRule[];  // Who can see this field
}

interface VisibilityRule {
  contactId: string;             // "*" for all, or specific contact ID
  canView: boolean;              // Permission flag
}

enum ContactFieldType {
  PHONE = "phone",
  EMAIL = "email",
  SOCIAL_TWITTER = "social_twitter",
  SOCIAL_INSTAGRAM = "social_instagram",
  SOCIAL_LINKEDIN = "social_linkedin",
  SOCIAL_FACEBOOK = "social_facebook",
  SOCIAL_GITHUB = "social_github",
  SOCIAL_OTHER = "social_other",
  ADDRESS = "address",
  WEBSITE = "website",
  CUSTOM = "custom"
}
```

### 3. Proximity Exchange Protocol

Physical proximity is verified through multiple mechanisms:

#### Option A: QR Code + Audio Verification (Primary)
1. User A displays QR code containing:
   - Public key
   - One-time exchange token
   - Short audio challenge
2. User B scans QR code
3. Both devices emit/listen for ultrasonic audio handshake
4. Audio verification confirms physical proximity (prevents remote QR scanning)

#### Option B: Bluetooth Low Energy (BLE)
1. Both devices advertise availability
2. RSSI (signal strength) used to verify proximity (<2m)
3. Public keys exchanged over BLE
4. Challenge-response to prevent relay attacks

#### Option C: NFC (Near Field Communication)
1. Devices must be within centimeters
2. Exchange public keys and signed tokens
3. Most secure for proximity but requires NFC hardware

#### Exchange Protocol Flow
```
User A                                    User B
   │                                         │
   │──── Display QR (pubkey + token) ────────│
   │                                         │
   │◄──── Scan QR, extract pubkey ───────────│
   │                                         │
   │──── Emit ultrasonic challenge ──────────│
   │                                         │
   │◄──── Respond with signed challenge ─────│
   │                                         │
   │──── X3DH Key Agreement ─────────────────│
   │                                         │
   │◄─── Exchange encrypted contact cards ───│
   │                                         │
   │──── Store contact, establish sync ──────│
   │                                         │
```

### 4. Sync & Update Propagation

#### Decentralized Sync Architecture

Using **libp2p** with DHT (Distributed Hash Table):

1. **Discovery**: Contacts find each other via DHT using public key hashes
2. **Connection**: Direct P2P connection when both online
3. **Relay**: Use relay nodes when direct connection impossible (NAT, etc.)
4. **Sync Protocol**:
   - CRDT-based (Conflict-free Replicated Data Types) for eventual consistency
   - Only encrypted deltas transmitted
   - Merkle tree for efficient change detection

#### Update Flow
```
┌──────────────┐                         ┌──────────────┐
│   User A     │                         │   User B     │
│  (updater)   │                         │  (contact)   │
└──────┬───────┘                         └──────┬───────┘
       │                                        │
       │  1. Modify contact field               │
       │                                        │
       │  2. Check visibility rules             │
       │     (B can see this field?)            │
       │                                        │
       │  3. Encrypt update with                │
       │     A-B shared key                     │
       │                                        │
       │  4. Sign update                        │
       │                                        │
       │──────── 5. Push to DHT ───────────────►│
       │         (or direct P2P)                │
       │                                        │
       │                          6. Receive    │
       │                             encrypted  │
       │                             update     │
       │                                        │
       │                          7. Verify     │
       │                             signature  │
       │                                        │
       │                          8. Decrypt &  │
       │                             apply      │
       │                                        │
```

### 5. Storage Layer

#### Local Storage (Per Device)
```
/webbook_data/
├── identity/
│   ├── master_key.enc          # Encrypted with device key
│   └── keypairs.enc            # Identity and exchange keys
├── contacts/
│   ├── {contact_id}.enc        # Encrypted contact cards
│   └── index.enc               # Encrypted contact index
├── my_card/
│   └── card.enc                # User's own contact card
└── sync/
    ├── pending_updates.enc     # Queue of outgoing updates
    └── merkle_state.enc        # Sync state tracking
```

#### Device Sync (Same User, Multiple Devices)
- Use **device linking** via QR code scan between user's devices
- Derive device-specific keys from master seed
- Sync encrypted vault between user's own devices

### 6. Voluntary Relay Network

For users behind restrictive NATs or firewalls:

```
┌─────────────────────────────────────────────────────────┐
│                    RELAY NODE                            │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │              Encrypted Blob Store                │    │
│  │   (No access to plaintext, just routing)         │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│  Features:                                               │
│  - Store-and-forward encrypted messages                 │
│  - No user accounts required                            │
│  - Rate limiting to prevent abuse                       │
│  - Automatic blob expiration (7 days)                   │
│  - Tor-friendly (.onion addresses)                      │
│                                                          │
│  Contribution Model:                                     │
│  - Docker image for easy deployment                     │
│  - Bandwidth tracking (optional donation prompt)        │
│  - No special privileges for contributors               │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

---

## Technology Stack

### Core Library (Shared)
- **Language**: Rust (compiled to native + WebAssembly)
- **Crypto**: libsodium / ring
- **P2P**: libp2p (rust-libp2p)
- **Storage**: SQLCipher (encrypted SQLite)
- **Serialization**: Protocol Buffers or MessagePack

### Mobile Apps
- **iOS**: Swift UI + Rust FFI
- **Android**: Kotlin + Rust JNI

### Desktop Apps
- **Framework**: Tauri (Rust backend + Web frontend)
- **Frontend**: SolidJS or Svelte (lightweight, performant)
- **Platforms**: Windows, macOS, Linux

### Web App (Optional)
- **Runtime**: WebAssembly (shared Rust core)
- **Storage**: IndexedDB (encrypted)
- **Limitations**: No BLE/NFC, QR-only exchange

---

## Security Considerations

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Remote contact harvesting | Physical proximity verification required |
| Man-in-the-middle | End-to-end encryption with verified keys |
| Server compromise | Server only sees encrypted blobs |
| Device theft | Local encryption with device key |
| Metadata leakage | Minimal metadata, random padding |
| Replay attacks | Timestamps and nonces in all messages |
| Key compromise | Forward secrecy via ratcheting |

### Data Classification
- **Highly Sensitive**: Private keys, master seed
- **Sensitive**: Contact information, visibility rules
- **Semi-Public**: Public keys, relay routing info

### Audit Requirements
- Regular security audits of crypto implementation
- Open source for community review
- Bug bounty program

---

## Scalability Considerations

### Horizontal Scaling
- No central database to scale
- DHT naturally distributes load
- Relay nodes can be added independently

### Performance Targets
- Contact exchange: < 3 seconds
- Update propagation: < 30 seconds (when online)
- Local operations: < 100ms
- App startup: < 2 seconds

### Data Limits
- Max contact card size: 64KB (encrypted)
- Max contacts per user: 10,000
- Max fields per card: 100

---

## Development Phases

### Phase 1: Foundation
- [ ] Core crypto library in Rust
- [ ] Data model and storage layer
- [ ] Basic CLI for testing

### Phase 2: Exchange Protocol
- [ ] QR code generation/scanning
- [ ] Audio proximity verification
- [ ] BLE exchange (mobile only)
- [ ] Key exchange protocol

### Phase 3: Sync Layer
- [ ] libp2p integration
- [ ] DHT-based discovery
- [ ] Update propagation
- [ ] Conflict resolution (CRDT)

### Phase 4: Mobile Apps
- [ ] iOS app (Swift UI)
- [ ] Android app (Kotlin)
- [ ] Platform-specific features (NFC, etc.)

### Phase 5: Desktop Apps
- [ ] Tauri application shell
- [ ] Cross-platform UI
- [ ] Device linking

### Phase 6: Infrastructure
- [ ] Relay node implementation
- [ ] Docker deployment
- [ ] Monitoring and health checks

### Phase 7: Polish
- [ ] UI/UX refinement
- [ ] Accessibility
- [ ] Localization
- [ ] Security audit

---

## API Design (Core Library)

```rust
// Identity Management
pub fn create_identity() -> Result<Identity, Error>;
pub fn export_identity_backup(password: &str) -> Result<EncryptedBackup, Error>;
pub fn import_identity_backup(backup: &EncryptedBackup, password: &str) -> Result<Identity, Error>;

// Contact Card Management
pub fn get_my_card() -> Result<ContactCard, Error>;
pub fn update_my_card(updates: CardUpdates) -> Result<ContactCard, Error>;
pub fn add_field(field: ContactField) -> Result<FieldId, Error>;
pub fn remove_field(field_id: &FieldId) -> Result<(), Error>;
pub fn set_field_visibility(field_id: &FieldId, rules: Vec<VisibilityRule>) -> Result<(), Error>;

// Contact Management
pub fn get_contacts() -> Result<Vec<Contact>, Error>;
pub fn get_contact(contact_id: &ContactId) -> Result<Contact, Error>;
pub fn remove_contact(contact_id: &ContactId) -> Result<(), Error>;
pub fn block_contact(contact_id: &ContactId) -> Result<(), Error>;

// Exchange
pub fn generate_exchange_qr() -> Result<QRData, Error>;
pub fn process_scanned_qr(qr_data: &QRData) -> Result<ExchangeSession, Error>;
pub fn complete_exchange(session: ExchangeSession) -> Result<Contact, Error>;

// Sync
pub fn start_sync_service() -> Result<SyncHandle, Error>;
pub fn stop_sync_service(handle: SyncHandle) -> Result<(), Error>;
pub fn get_sync_status() -> Result<SyncStatus, Error>;

// Device Management
pub fn link_device(qr_data: &QRData) -> Result<DeviceLink, Error>;
pub fn get_linked_devices() -> Result<Vec<Device>, Error>;
pub fn unlink_device(device_id: &DeviceId) -> Result<(), Error>;
```

---

## Glossary

- **Contact Card**: A user's collection of contact information
- **Field**: A single piece of contact information (phone, email, etc.)
- **Visibility Rule**: Permission setting for who can see a field
- **Exchange**: The process of sharing contact cards in person
- **Relay Node**: Volunteer-run server for store-and-forward delivery
- **DHT**: Distributed Hash Table for peer discovery
- **X3DH**: Extended Triple Diffie-Hellman key agreement
- **CRDT**: Conflict-free Replicated Data Type for sync

# Device Linking

Device linking enables users to use the same WebBook identity across multiple devices with full sync of contacts, cards, and settings.

## Goals

1. **Seamless Multi-Device**: Same identity on phone, desktop, tablet
2. **Secure Transfer**: Master seed encrypted during device pairing
3. **Forward Secrecy**: Each device maintains independent ratchets with contacts
4. **Device Revocation**: Compromise of one device doesn't expose all data

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         IDENTITY LAYER                               │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    Master Seed (32 bytes)                    │    │
│  │                 (shared across all devices)                  │    │
│  └───────────────────────┬─────────────────────────────────────┘    │
│                          │                                           │
│            ┌─────────────┼─────────────┐                            │
│            ▼             ▼             ▼                            │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                 │
│  │   Device 0   │ │   Device 1   │ │   Device 2   │                 │
│  │   (Phone)    │ │  (Desktop)   │ │   (Tablet)   │                 │
│  └──────────────┘ └──────────────┘ └──────────────┘                 │
│        │                 │                 │                        │
│  Derived Keys:     Derived Keys:     Derived Keys:                  │
│  - device_id       - device_id       - device_id                    │
│  - exchange_key    - exchange_key    - exchange_key                 │
└─────────────────────────────────────────────────────────────────────┘
```

## Key Derivation

Each device derives its unique keys from the master seed:

```
device_id[n]       = HKDF(master_seed, n, "WebBook_Device_ID")
exchange_key[n]    = HKDF(master_seed, n, "WebBook_Device_Exchange")
```

This ensures:
- **Deterministic**: Same seed + index always produces same keys
- **Unique**: Different devices have different communication keys
- **Verifiable**: Can regenerate device keys from seed if needed

## Device Registry

The device registry tracks all linked devices:

```rust
struct DeviceRegistry {
    devices: Vec<RegisteredDevice>,
    version: u64,
    signature: [u8; 64],  // Signed by identity key
}

struct RegisteredDevice {
    device_id: [u8; 32],
    exchange_public_key: [u8; 32],
    device_name: String,
    created_at: u64,
    revoked: bool,
    revoked_at: Option<u64>,
}
```

Key properties:
- **Signed**: Registry is signed by identity signing key
- **Versioned**: Monotonic version for conflict resolution
- **Revocable**: Devices can be revoked without deleting

## Device Linking Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│  DEVICE A (Existing)                    DEVICE B (New)               │
│                                                                       │
│  1. Generate link_key                                                │
│  2. Display QR:                                                      │
│     ┌─────────────────────┐                                          │
│     │ WBDL | v1           │                                          │
│     │ identity_pubkey     │────────────────────────────────────────► │
│     │ link_key            │              3. Scan QR                  │
│     │ timestamp           │                                          │
│     │ signature           │                                          │
│     └─────────────────────┘                                          │
│                                                                       │
│                              ◄───────────────────────────────────────│
│  4. Receive request         │  DeviceLinkRequest (encrypted):       │
│                              │  - device_name                        │
│                              │  - nonce                              │
│                              │  - timestamp                          │
│                                                                       │
│  5. Assign device_index                                              │
│  6. Update registry                                                  │
│                                                                       │
│  DeviceLinkResponse         │                                        │
│  (encrypted):               │────────────────────────────────────────►
│  - master_seed              │  7. Receive response                   │
│  - display_name             │  8. Derive keys from seed              │
│  - device_index             │  9. Store identity + registry          │
│  - registry                 │                                        │
│                                                                       │
│  10. Broadcast registry update to contacts                           │
└─────────────────────────────────────────────────────────────────────┘
```

## QR Code Format

The device link QR contains:

| Field | Size | Description |
|-------|------|-------------|
| Magic | 4 bytes | `WBDL` (WebBook Device Link) |
| Version | 1 byte | Protocol version (1) |
| Identity Key | 32 bytes | Ed25519 public key |
| Link Key | 32 bytes | Random encryption key |
| Timestamp | 8 bytes | Unix timestamp |
| Signature | 64 bytes | Ed25519 signature |

Total: 141 bytes, base64 encoded for QR display.

## Security Properties

### Confidentiality
- Master seed is encrypted with link_key before transmission
- Link key is only known to the two devices (displayed/scanned)
- All communication after linking uses end-to-end encryption

### Integrity
- QR is signed by identity key (proves ownership)
- Registry is signed (prevents tampering)
- Request/response have nonces (prevent replay)

### Physical Proximity
- QR code requires camera/in-person exchange
- 10-minute expiry on link QR codes
- New device gets unique index (not replayable)

### Device Compromise
- Revoking a device removes it from registry
- Contacts notified of revocation
- Forward secrecy: past messages still protected

## Per-Device Ratchets

When syncing with contacts, each device maintains independent Double Ratchet state:

```
┌─────────────┐     ┌─────────────────────────────────────┐
│   Contact   │     │            Your Devices              │
│    "Bob"    │     ├─────────────┬─────────────┬─────────┤
│             │     │  Device 0   │  Device 1   │ Device 2│
│             │─────│  Ratchet_0  │  Ratchet_1  │Ratchet_2│
└─────────────┘     └─────────────┴─────────────┴─────────┘
```

This provides:
- **Independent Forward Secrecy**: Compromise of one device's ratchet doesn't affect others
- **Parallel Sync**: All your devices receive updates from contacts
- **Graceful Revocation**: Contacts stop sending to revoked devices

## Inter-Device Sync

Your own devices sync with each other using device-to-device ratchets:

```
Device 0 ◄──── Ratchet ────► Device 1
    │                           │
    └──── Ratchet ────► Device 2
```

Synced data:
- Contact additions/removals
- Card updates
- Settings changes
- Registry updates

Conflict resolution: Last-write-wins with version numbers.

## Implementation Status

| Component | Status |
|-----------|--------|
| DeviceInfo struct | Complete |
| DeviceRegistry | Complete |
| Device key derivation (HKDF) | Complete |
| DeviceLinkQR generation | Complete |
| DeviceLinkRequest/Response | Complete |
| Device linking flow | Complete |
| Storage tables | Complete |
| Backup/restore with device_index | Complete |
| Per-device contact ratchets | Planned |
| Inter-device sync | Planned |
| Device revocation broadcast | Planned |

## Files

| File | Description |
|------|-------------|
| `webbook-core/src/identity/device.rs` | DeviceInfo, DeviceRegistry, key derivation |
| `webbook-core/src/exchange/device_link.rs` | Device linking protocol |
| `webbook-core/src/storage/mod.rs` | Device storage tables |

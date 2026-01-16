# Storage Layer

## Local Storage (Per Device)

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

## Device Sync (Same User, Multiple Devices)

- Use **device linking** via QR code scan between user's devices
- Derive device-specific keys from master seed
- Sync encrypted vault between user's own devices

## Implementation

- **Database**: SQLite with encryption
- **Encryption**: AES-256-GCM for all stored data
- **Key Storage**: Platform secure enclave when available (Keystore on Android, Keychain on iOS)

<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Architecture

## Security Model

Vauchi uses established cryptographic protocols to ensure privacy:

- **X3DH** (Extended Triple Diffie-Hellman) - Establishes shared secrets during contact exchange without requiring both parties to be online simultaneously
- **Double Ratchet** - Provides forward secrecy and break-in recovery; each message uses a unique key
- **Ed25519** - Digital signatures for identity verification
- **AES-256-GCM** - Authenticated encryption for all stored and transmitted data

All cryptographic operations use the audited `ring` crate. See [docs/architecture/cryptography.md](../../docs/architecture/cryptography.md) for full specification.

## Data Flow

```
┌─────────────┐     QR Code      ┌─────────────┐
│   Alice     │ ───────────────► │    Bob      │
│  (Device)   │ ◄─────────────── │  (Device)   │
└─────────────┘    X3DH Keys     └─────────────┘
       │                                │
       │  Encrypted Updates             │
       ▼                                ▼
┌─────────────┐                 ┌─────────────┐
│   Relay     │ ◄─────────────► │   Relay     │
│   Server    │   Store & Fwd   │   Server    │
└─────────────┘                 └─────────────┘
```

1. **Exchange** - Users scan QR codes to exchange X3DH keys
2. **Encrypt** - Card updates are encrypted with Double Ratchet
3. **Relay** - Encrypted blobs pass through relay servers (zero knowledge)
4. **Decrypt** - Only the intended recipient can decrypt

## Key Components

### Identity
Each user has a master seed that deterministically derives:
- Ed25519 signing keypair (identity verification)
- X25519 exchange keypair (key agreement)

### Contact Cards
Structured data with typed fields (email, phone, etc.) and:
- Per-field visibility rules
- Delta-based sync (only changes transmitted)
- Signature verification

### Sync Protocol
- Offline-first with queue-based delivery
- Automatic retry with exponential backoff
- Acknowledgment tracking
- Update coalescing to reduce bandwidth

### Transport Layer
- Abstract `Transport` trait for platform implementations
- Connection management with auto-reconnect
- Message framing and versioning

# WebBook Architecture

WebBook is a privacy-focused, decentralized contact card exchange application that allows users to share and update contact information with people they meet in the physical world.

## Core Principles

1. **Privacy First**: All data is end-to-end encrypted
2. **Decentralized**: Minimize reliance on central servers
3. **Physical Proximity Required**: Contact exchange only happens in-person
4. **User Control**: Users decide what information each contact can see
5. **Real-time Updates**: Changes propagate to authorized contacts automatically

## High-Level Architecture

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

## Architecture Documents

| Document | Description |
|----------|-------------|
| [Cryptography](./cryptography.md) | Identity, key derivation, encryption schemes |
| [Data Models](./data-models.md) | Contact cards, fields, visibility rules |
| [Exchange Protocol](./exchange-protocol.md) | QR, BLE, NFC proximity verification |
| [Device Linking](./device-linking.md) | Multi-device sync and device pairing |
| [Sync](./sync.md) | Update propagation, P2P sync |
| [Storage](./storage.md) | Local encrypted storage, device sync |
| [Relay](./relay.md) | Relay network and federation |
| [Tech Stack](./tech-stack.md) | Languages, frameworks, platforms |
| [Security](./security.md) | Threat model, data classification |
| [API](./api.md) | Core library API design |

## Glossary

- **Contact Card**: A user's collection of contact information
- **Field**: A single piece of contact information (phone, email, etc.)
- **Visibility Rule**: Permission setting for who can see a field
- **Exchange**: The process of sharing contact cards in person
- **Relay Node**: Volunteer-run server for store-and-forward delivery
- **DHT**: Distributed Hash Table for peer discovery
- **X3DH**: Extended Triple Diffie-Hellman key agreement
- **CRDT**: Conflict-free Replicated Data Type for sync

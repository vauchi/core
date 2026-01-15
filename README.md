# WebBook

A privacy-focused platform for exchanging contact information that stays up-to-date.

## The Problem

When you exchange contact details with someone, that information becomes outdated the moment either of you changes your phone number, email, social media, or address. You end up with stale contacts, and people lose touch.

Worse, social media platforms keep users captive by implicitly threatening them with losing their contacts if they leave. Your relationships become locked inside platforms you may no longer want to use.

## The Solution

WebBook lets you exchange "living" contact cards. When you update your information, everyone you've shared it with automatically receives the update - securely and privately.

## Key Principles

- **In-Person Exchange** - Contact cards can only be exchanged when physically together (QR code scan)
- **Selective Sharing** - Control which contacts see which fields (work email vs personal)
- **No Messages** - This is not a messenger; it only syncs contact information
- **End-to-End Encrypted** - No server can read your data
- **Decentralized** - Relay servers only pass encrypted blobs; they have zero knowledge

## Project Structure

```
WebBook/
├── webbook-core/     # Core Rust library (cryptography, protocols, data models)
├── webbook-relay/    # WebSocket relay server for message forwarding
└── webbook-cli/      # Command-line interface for testing
```

### webbook-core

The core library implements all cryptographic protocols (X3DH, Double Ratchet), identity management, contact cards, and sync protocol. Platform-independent, ready for mobile integration via FFI.

See [webbook-core/README.md](webbook-core/README.md) for details.

### webbook-relay

Lightweight WebSocket relay server that stores and forwards encrypted blobs between clients. Zero-knowledge design - the server only sees encrypted data it cannot decrypt.

See [webbook-relay/README.md](webbook-relay/README.md) for details.

### webbook-cli

Command-line interface for testing and demonstration. Supports identity creation, contact card management, QR-based contact exchange, and synchronization via the relay server.

See [webbook-cli/README.md](webbook-cli/README.md) for details.

## Quick Start

```bash
# Start the relay server (in one terminal)
cargo run -p webbook-relay

# Create an identity (in another terminal)
cargo run -p webbook-cli -- init "Alice"

# Show your contact card
cargo run -p webbook-cli -- card show

# Generate exchange QR code
cargo run -p webbook-cli -- exchange start

# Sync with relay to receive messages
cargo run -p webbook-cli -- sync
```

## Planned Components

- **iOS App** - Native Swift app using webbook-core via FFI
- **Android App** - Native Kotlin app using webbook-core via FFI
- **Desktop Apps** - Cross-platform GUI for macOS, Windows, Linux

## License

MIT

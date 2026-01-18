# Vauchi

A privacy-focused platform for exchanging contact information that stays up-to-date.

## The Problem

When you exchange contact details with someone, that information becomes outdated the moment either of you changes your phone number, email, social media, or address. You end up with stale contacts, and people lose touch.

Worse, social media platforms keep users captive by implicitly threatening them with losing their contacts if they leave. Your relationships become locked inside platforms you may no longer want to use.

## The Solution

Vauchi lets you exchange "living" contact cards. When you update your information, everyone you've shared it with automatically receives the update - securely and privately.

## Key Principles

- **In-Person Exchange** - Contact cards can only be exchanged when physically together (QR code scan)
- **Selective Sharing** - Control which contacts see which fields (work email vs personal)
- **No Messages** - This is not a messenger; it only syncs contact information
- **End-to-End Encrypted** - No server can read your data
- **Decentralized** - Relay servers only pass encrypted blobs; they have zero knowledge

## Project Structure

```
Vauchi/
├── vauchi-core/     # Core Rust library (cryptography, protocols, data models)
├── vauchi-relay/    # WebSocket relay server for message forwarding
└── vauchi-cli/      # Command-line interface for testing
```

### vauchi-core

The core library implements all cryptographic protocols (X3DH, Double Ratchet), identity management, contact cards, and sync protocol. Platform-independent, ready for mobile integration via FFI.

See [vauchi-core/README.md](vauchi-core/README.md) for details.

### vauchi-relay

Lightweight WebSocket relay server that stores and forwards encrypted blobs between clients. Zero-knowledge design - the server only sees encrypted data it cannot decrypt.

See [vauchi-relay/README.md](vauchi-relay/README.md) for details.

### vauchi-cli

Command-line interface for testing and demonstration. Supports identity creation, contact card management, QR-based contact exchange, and synchronization via the relay server.

See [vauchi-cli/README.md](vauchi-cli/README.md) for details.

## Quick Start

```bash
cargo run -p vauchi-relay     # Start relay server (terminal 1)
cargo run -p vauchi-cli -- init "Alice"  # Create identity (terminal 2)
cargo run -p vauchi-cli -- sync          # Sync with relay
```

For full build commands and development workflow, see [CLAUDE.md](CLAUDE.md).

## Contributing

This project uses strict Test-Driven Development. Before contributing:

1. Read [CLAUDE.md](CLAUDE.md) for project structure and commit rules
2. Read [docs/TDD_RULES.md](docs/TDD_RULES.md) for the TDD workflow
3. Read [docs/architecture/](docs/architecture/) for technical design

## Planned Components

- **iOS App** - Native Swift app using vauchi-core via FFI
- **Android App** - Native Kotlin app using vauchi-core via FFI
- **Desktop Apps** - Cross-platform GUI for macOS, Windows, Linux

## License

MIT

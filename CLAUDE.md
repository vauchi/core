# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

WebBook is a privacy-focused, decentralized contact card exchange application. Users exchange contact cards only through physical proximity (in-person), and can control what contact information others see.

## Project Structure

```
WebBook/
├── webbook-core/     # Core Rust library (cryptography, data models, protocols)
├── webbook-relay/    # WebSocket relay server for message forwarding
└── webbook-cli/      # Command-line interface for testing and demonstration
```

## Build & Test Commands

```bash
# Run all tests (all crates)
cargo test

# Run tests for specific crate
cargo test -p webbook-core
cargo test -p webbook-relay
cargo test -p webbook-cli

# Run CLI commands
cargo run -p webbook-cli -- --help
cargo run -p webbook-cli -- init "Your Name"
cargo run -p webbook-cli -- sync

# Start relay server
cargo run -p webbook-relay

# Check code quality
cargo clippy -- -D warnings

# Format code
cargo fmt
```

## Architecture

The project uses a **shared Rust core library** (`webbook-core/`) that will be compiled to native code for mobile (via FFI) and WebAssembly for web/desktop. The relay server and CLI are separate crates that depend on the core.

### webbook-core Modules

- `crypto/` - Cryptographic primitives (Ed25519, X25519, AES-256-GCM, X3DH, Double Ratchet)
- `identity/` - User identity management and encrypted backup/restore
- `contact_card/` - Contact card and field management with validation
- `contact/` - Contact storage with shared secrets
- `exchange/` - QR code generation for contact exchange
- `sync/` - Update propagation protocol
- `storage/` - Encrypted local storage
- `network/` - Transport abstractions

### webbook-relay

- WebSocket server using tokio-tungstenite
- In-memory blob storage with automatic expiration
- Rate limiting per client
- Zero-knowledge design (only sees encrypted blobs)

### webbook-cli

- Full CLI for identity, card, contact, and exchange management
- Real WebSocket sync with relay server
- QR code generation for contact exchange

### Key Design Decisions

- **Crypto**: Uses `ring` crate (audited, production-ready) - never implement custom crypto
- **Encryption**: AES-256-GCM with random nonces, PBKDF2 for password-derived keys
- **Signing**: Ed25519 for identity and message signatures
- **Key Exchange**: X3DH for initial contact exchange
- **Forward Secrecy**: Double Ratchet for update encryption
- **Memory Safety**: Sensitive data (seeds, keys) zeroed on drop via `zeroize`

## Development Rules

### TDD Workflow (Mandatory)

Follow strict Test-Driven Development as defined in `docs/TDD_RULES.md`:

1. **RED**: Write failing test first based on Gherkin scenarios in `features/`
2. **GREEN**: Write minimal code to pass the test
3. **REFACTOR**: Improve code while keeping tests green
4. **COMMIT**: When all tests pass, create a commit

### Commit Rule

**Every time all tests are green, create a commit.** This ensures:
- Progress is captured incrementally
- Easy rollback if issues arise
- Clear history of TDD cycles

### Test Requirements

- Minimum 90% code coverage for core library
- All Gherkin scenarios must have corresponding tests
- Never mock cryptographic operations - use real crypto in tests

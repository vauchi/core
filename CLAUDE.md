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

### Commit Rules

1. **Test-first**: Every time all tests are green, create a commit
2. **Documentation Updates**: Before committing, update relevant documentation if:
   - New features → Update `docs/MVP_PLAN.md` status, add Gherkin scenarios to `features/`
   - Architecture changes → Update `docs/ARCHITECTURE.md`
   - API changes → Update relevant crate README (`webbook-core/`, `webbook-cli/`, `webbook-relay/`, `webbook-mobile/`)
3. **Planning Documentation**: For significant features, create/update a plan document linking to:
   - Related Gherkin feature files
   - Architecture decisions
   - Implementation approach
4. **Feature Descriptions**: New user-facing features require:
   - Gherkin scenarios in `features/*.feature` (always first - TDD)
   - Brief description in `docs/MVP_PLAN.md`

### Test Requirements

- Minimum 90% code coverage for core library
- All Gherkin scenarios must have corresponding tests
- Never mock cryptographic operations - use real crypto in tests

## Documentation Index

| Document | Purpose | Update When |
|----------|---------|-------------|
| `CLAUDE.md` | AI assistant quick reference | Build commands or project structure changes |
| `README.md` | GitHub visitors intro | Project description or quick start changes |
| `docs/ARCHITECTURE.md` | Technical design details | Architecture, protocols, or data model changes |
| `docs/MVP_PLAN.md` | Current status and roadmap | Feature completion or roadmap changes |
| `docs/TDD_RULES.md` | Development methodology | Testing process changes |
| `docs/THREAT_ANALYSIS.md` | Security threat model | Security-relevant changes |
| `features/*.feature` | Gherkin scenarios | Before implementing any new feature (TDD) |
| `webbook-*/README.md` | Crate-specific docs | Crate API or usage changes |

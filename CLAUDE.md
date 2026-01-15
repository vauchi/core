# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

WebBook is a privacy-focused, decentralized contact card exchange application. Users exchange contact cards only through physical proximity (in-person), and can control what contact information others see.

## Build & Test Commands

```bash
# Run all tests
cargo test

# Run tests for specific module
cargo test crypto
cargo test identity
cargo test contact_card

# Check code quality
cargo clippy -- -D warnings

# Format code
cargo fmt
```

## Architecture

The project uses a **shared Rust core library** (`webbook-core/`) that will be compiled to native code for mobile (via FFI) and WebAssembly for web/desktop.

### Module Structure

- `crypto/` - Cryptographic primitives (Ed25519 signatures, AES-256-GCM encryption)
- `identity/` - User identity management and encrypted backup/restore
- `contact_card/` - Contact card and field management with validation

### Key Design Decisions

- **Crypto**: Uses `ring` crate (audited, production-ready) - never implement custom crypto
- **Encryption**: AES-256-GCM with random nonces, PBKDF2 for password-derived keys
- **Signing**: Ed25519 for identity and message signatures
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

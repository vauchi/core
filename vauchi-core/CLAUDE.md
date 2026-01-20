# CLAUDE.md - vauchi-core

Core cryptographic library. All crypto primitives, protocols, and data models.

## Rules

- **No external crate dependencies** from other workspace crates
- **Crypto**: `ring` only. Never mock crypto in tests
- **Coverage**: 90%+ required
- **No unsafe**: Avoid `unsafe` unless absolutely necessary with documentation

## Testing

```bash
cargo test -p vauchi-core
cargo test -p vauchi-core -- --nocapture  # See output
```

## Structure

- `src/` - Production code
- `tests/` - Integration tests (reference `../../features/*.feature`)

## Security Critical

This crate handles:
- Key generation and management
- E2E encryption/decryption
- Cryptographic signatures
- Protocol state machines

Review all changes carefully. Security bugs here affect all clients.

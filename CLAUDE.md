# CLAUDE.md - vauchi-core

> **Inherits**: See [/CLAUDE.md](/CLAUDE.md) for project-wide rules.
> **Reference**: [/PRINCIPLES.md](/PRINCIPLES.md), [TDD Rules](/_docs/2026-01-22-TDD_RULES.md)

Core library and mobile bindings for Vauchi - privacy-focused updatable contact cards.

See [README.md](README.md) for overview.

## Component-Specific Rules

- **Crypto**: `ring` only. No custom crypto. No mocking crypto.
- **Coverage**: 90%+ for vauchi-core.
- **Planning docs**: Feature complete → MUST update original `/_docs/planning/todo/` doc and move to `done/`.

## Commands

```bash
cargo test --workspace          # All tests
cargo test -p vauchi-core       # Core tests only
cargo test -p vauchi-mobile     # Mobile bindings tests
cargo clippy -- -D warnings     # Lint (must pass)
cargo fmt                       # Format
```

## Crates

| Crate | Purpose |
|-------|---------|
| vauchi-core | Crypto, protocols, data models |
| vauchi-mobile | UniFFI bindings for iOS/Android |

## Downstream Repos

These depend on vauchi-core via git dependency:
- `relay/` - WebSocket relay server
- `cli/` - Command-line interface
- `tui/` - Terminal UI
- `desktop/` - Tauri + SolidJS desktop app
- `e2e/` - End-to-end tests

## Commits

All tests green. Update: `/features/` for features, README for API changes.

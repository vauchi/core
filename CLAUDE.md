# CLAUDE.md - vauchi-core

> **Inherits**: See [/CLAUDE.md](/CLAUDE.md) for project-wide rules.

Core library and mobile bindings. Crypto, protocols, data models, UniFFI bindings.

## Rules

- **Crypto**: `ring` only. No custom crypto. No mocking crypto.
- **Coverage**: 90%+ for vauchi-core.
- **Crates**: `vauchi-core` (crypto, protocols), `vauchi-mobile` (UniFFI), `vauchi-protocol` (shared relay/client types, serde-only)
- **Downstream**: cli, tui, desktop, e2e depend on vauchi-core. relay depends on vauchi-protocol only.
- **NFC**: Two planned features (0% implemented, post-MVP): "NFC Active" (phone-to-phone) and "NFC Dead Drop" (passive tag). See problem records in `_private/docs/problems/`.

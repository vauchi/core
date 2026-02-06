<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# CLAUDE.md - vauchi-core

> **Inherits**: See [/CLAUDE.md](/CLAUDE.md) for project-wide rules.
> **Reference**: [Principles](https://docs.vauchi.app/about/principles/), [TDD Rules](https://docs.vauchi.app/developers/tdd-rules/)

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
- `cli/` - Command-line interface
- `tui/` - Terminal UI
- `desktop/` - Tauri + SolidJS desktop app
- `e2e/` - End-to-end tests

Note: `relay/` is standalone and does **not** depend on vauchi-core.

## NFC Exchange Scenarios

Two distinct NFC features exist. Use these names consistently:

### "NFC Active" (phone-to-phone tap)

- **Shorthand**: NFC Active, active NFC, device-to-device NFC
- **Problem record**: `_private/docs/problems/2026-02-02-nfc-active-device-exchange/`
- **Status**: `planning` (P2, post-MVP) — **0% code implemented**
- **Planned module**: `vauchi-core/src/exchange/nfc_active.rs`
- **Magic bytes**: `VNFC`
- **Scenario**: Two NFC-capable smartphones tap together. Both devices are active. NFC replaces both QR scan (transport) and ultrasonic audio (proximity verification) in a single tap.
- **Key design**: Fresh ephemeral X25519 keys on both sides (full forward secrecy), APDU over HCE, reuses `ExchangeSession` state machine with `AwaitingNfcTap` state
- **Platform**: Android<->Android (full), iOS->Android (full), iOS<->iOS (impossible — falls back to QR)
- **No relay involvement** (both devices present)
- **Error prefix**: `Nfc*` (e.g. `InvalidNfcFormat`, `NfcExpired`, `NfcSessionLost`)

### "NFC Dead Drop" (passive tag + active phone)

- **Shorthand**: NFC Dead Drop, dead drop, NFC tag, passive NFC
- **Problem record**: `_private/docs/problems/2026-02-02-nfc-dead-drop-exchange/`
- **Status**: `investigating` (P2, post-MVP) — **0% code implemented**
- **Planned module**: `vauchi-core/src/exchange/nfc_tag.rs`
- **Magic bytes**: `VDDP`
- **Scenario**: One user has a passive NFC tag (NTAG215/216), the other taps it with their phone. Asynchronous — write and read happen at different times.
- **Key design**: Zone A (locked, 182B: identity + exchange key + password verifier + signature) + Zone B (writable, 280B: encrypted introduction). Password-protected. PBKDF2 verification + XChaCha20-Poly1305 encryption. HKDF key derivation with optional password mixing.
- **Platform**: Any NFC phone + passive NTAG215 (504B) or NTAG216 (888B)
- **Return path**: Existing relay `EncryptedUpdate` (zero relay changes)
- **Error prefix**: `Tag*` (e.g. `InvalidTagFormat`, `TagPasswordFailed`, `TagZoneFull`)

### History

Both scenarios were originally bundled in a single "NFC tag device" feature that introduced a relay "mailbox" concept. This was removed in vauchi/core!49 for violating the zero-knowledge relay principle. The two scenarios were split into separate problem records on 2026-02-02.

## Commits

All tests green. Update: `/features/` for features, README for API changes.

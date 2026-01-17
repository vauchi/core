# CLAUDE.md

Privacy-focused updatable contact card exchange via trusted in real life meetings. Users control what each contact sees.

## ⚠️ MANDATORY — STOP IF VIOLATED

**TDD**: Red→Green→Refactor. Test FIRST or delete code and restart. See `docs/TDD_RULES.md`. Tests trace to `features/*.feature`.

**Structure**: `src/` = production only. `tests/` = tests only. Siblings. Configure languages as needed.

**Planning docs**: Feature complete → MUST update original `docs/planning/todo/` doc and move to `done/`. Non-negotiable.

**Crypto**: `ring` only. No custom crypto. No mocking crypto.

**Coverage**: 90%+ for webbook-core.

**Fail fast**: Riskiest first. Return errors immediately. Use `Result`/`Option`.

## Commands

```bash
cargo test                        # all tests
cargo test -p webbook-core        # specific crate
cargo clippy -- -D warnings       # lint
cargo fmt                         # format
cargo run -p webbook-relay        # relay server
cargo run -p webbook-cli -- help  # CLI
```

## Structure

```
webbook-core/     # core lib (crypto, storage, sync)
webbook-relay/    # WebSocket relay
webbook-cli/      # CLI
webbook-tui/      # Terminal UI (ratatui)
webbook-desktop/  # Desktop (Tauri + SolidJS)
webbook-mobile/   # UniFFI bindings
webbook-android/  # Android (Kotlin/Compose)
webbook-ios/      # iOS (SwiftUI)
features/         # Gherkin scenarios
docs/             # Architecture, planning, docs
scripts/          # Build/utility scripts
```

## Commits

All tests green. Update: `features/` for features, `docs/architecture/` for arch, crate README for API.

## Docs

`docs/architecture/`, `docs/planning/{done,todo}/`, `docs/TDD_RULES.md`, `docs/THREAT_ANALYSIS.md`
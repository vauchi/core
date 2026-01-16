# CLAUDE.md

Privacy-focused contact card exchange app. Users exchange cards in-person via QR and control what each contact sees.

## Commands

```bash
cargo test                        # Run all tests
cargo test -p webbook-core        # Test specific crate
cargo clippy -- -D warnings       # Lint
cargo fmt                         # Format
cargo run -p webbook-relay        # Start relay server
cargo run -p webbook-cli -- help  # CLI help
```

## Project Structure

```
webbook-core/     # Rust core library (crypto, storage, sync)
webbook-relay/    # WebSocket relay server
webbook-cli/      # CLI tool
webbook-mobile/   # UniFFI bindings for iOS/Android
webbook-android/  # Android app (Kotlin/Compose)
features/         # Gherkin scenarios
```

## Coding Rules

**TDD Required** - Follow `docs/TDD_RULES.md`:
1. Write failing test first (from `features/*.feature`)
2. Write minimal code to pass
3. Refactor, keep tests green
4. Commit when tests pass

**Crypto** - Use `ring` crate only. Never implement custom crypto. Never mock crypto in tests.

**Test Coverage** - Minimum 90% for webbook-core.

## Commit Rules

1. Commit when all tests pass
2. Update docs before committing:
   - New features → `docs/planning/`, `features/*.feature`
   - Architecture changes → `docs/architecture/`
   - API changes → crate README
3. Significant features need a plan in `docs/planning/todo/`

## Key Docs

- `docs/architecture/` - Technical design
- `docs/planning/done/` - Completed work
- `docs/planning/todo/` - Planned work
- `docs/TDD_RULES.md` - TDD methodology
- `docs/THREAT_ANALYSIS.md` - Security threats

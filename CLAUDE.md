# CLAUDE.md

## ⚠️ MANDATORY: Test-Driven Development

**YOU MUST FOLLOW TDD. This is non-negotiable.**

Before writing ANY production code:
1. **RED**: Write a failing test first. Run it. Confirm it fails.
2. **GREEN**: Write the minimal code to make it pass. Run tests.
3. **REFACTOR**: Clean up while keeping tests green.

**STOP and check yourself**: Did you write a test first? If not, delete your production code and start over with a test.

See `docs/TDD_RULES.md` for full methodology. Tests come from `features/*.feature` Gherkin scenarios.

---

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

**TDD Workflow** (for EVERY new function/feature):
```
1. cargo test                    # Baseline - all pass
2. [Write new test]              # Must reference a feature/*.feature scenario
3. cargo test                    # Verify test FAILS (RED)
4. [Write minimal implementation]
5. cargo test                    # Verify test PASSES (GREEN)
6. [Refactor if needed]
7. cargo test                    # Still green
8. git commit                    # Only when green
```

**If you catch yourself writing code before a test**: STOP. Delete the code. Write the test first.

**Crypto** - Use `ring` crate only. Never implement custom crypto. Never mock crypto in tests.

**Test Coverage** - Minimum 90% for webbook-core.

**Fail Fast**
- When there is a choice, start with most difficult or riskiest things.
- Return errors immediately. Validate at boundaries. Use `Result`/`Option`, never silently ignore failures.


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

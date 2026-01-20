# CLAUDE.md

Privacy-focused updatable contact card exchange via trusted in real life meetings. Users control what each contact sees.

See [README.md](README.md) for overview, [COMMANDS.md](COMMANDS.md) for all commands.

## ⚠️ MANDATORY — STOP IF VIOLATED

**TDD**: Red→Green→Refactor. Test FIRST or delete code and restart. See `docs/TDD_RULES.md`. Tests trace to `../features/*.feature`.

**Structure**: `src/` = production only. `tests/` = tests only. Siblings. Configure languages as needed.

**Planning docs**: Feature complete → MUST update original `docs/planning/todo/` doc and move to `done/`. Non-negotiable.

**Crypto**: `ring` only. No custom crypto. No mocking crypto.

**Coverage**: 90%+ for vauchi-core.

**Fail fast**: Riskiest first. Return errors immediately. Use `Result`/`Option`.

## Commands

```bash
cargo test --workspace          # All tests
cargo test -p <crate>           # Specific crate
cargo clippy -- -D warnings     # Lint (must pass)
cargo fmt                       # Format
```

## Crate Dependencies

```
vauchi-core (standalone, no workspace deps)
    ↑
vauchi-relay, vauchi-cli, vauchi-mobile, vauchi-desktop, vauchi-tui
```

## Commits

All tests green. Update: `../features/` for features, crate README for API changes.
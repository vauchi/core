# CLAUDE.md - vauchi-cli

Command-line interface for testing and development.

## Rules

- CLI is for testing/dev, not end-user facing
- Depends on `vauchi-core`

## Commands

```bash
cargo run -p vauchi-cli -- init "Name"      # Initialize identity
cargo run -p vauchi-cli -- --help           # Show help
cargo test -p vauchi-cli                    # Run tests
```

## Usage

Primarily used for manual testing of core functionality without mobile/desktop UI.

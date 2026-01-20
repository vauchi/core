# Commands Reference

All commands run from `code/` directory.

## Build & Test

```bash
cargo build                     # Build all crates
cargo build --release           # Release build
cargo test --workspace          # Run all tests
cargo test -p <crate>           # Test specific crate
cargo test -- --nocapture       # Show test output
cargo test <pattern>            # Run matching tests
```

## Code Quality

```bash
cargo fmt                       # Format all code
cargo fmt --check               # Check formatting
cargo clippy -- -D warnings     # Lint (must pass)
cargo clippy --fix              # Auto-fix lint issues
```

## Documentation

```bash
cargo doc --no-deps --open      # Generate and view docs
cargo doc --document-private-items  # Include private items
```

## Run Applications

```bash
# CLI
cargo run -p vauchi-cli -- init "Name"
cargo run -p vauchi-cli -- --help

# TUI
cargo run -p vauchi-tui

# Relay Server
cargo run -p vauchi-relay
RUST_LOG=debug cargo run -p vauchi-relay  # With debug logging

# Desktop (Tauri)
cd vauchi-desktop && cargo tauri dev
cd vauchi-desktop && cargo tauri build
```

## Mobile Bindings

```bash
# Generate UniFFI bindings (from workspace root)
../dev-tools/scripts/build-bindings.sh

# Build for Android
../dev-tools/scripts/build-android.sh

# Build for iOS (macOS only)
cd ../ios && ./build-ios.sh
```

## Testing Helpers

```bash
# Multi-instance testing (from dev-tools/scripts/dev/)
./local-relay.sh                # Start local relay
./multi-cli.sh alice init "Alice"  # Run CLI as "alice"
./multi-tui.sh bob              # Run TUI as "bob"
```

## Coverage

```bash
# Requires cargo-tarpaulin
cargo tarpaulin -p vauchi-core --out Html
```

## Crate-Specific

| Crate | Test Command |
|-------|--------------|
| vauchi-core | `cargo test -p vauchi-core` |
| vauchi-relay | `cargo test -p vauchi-relay` |
| vauchi-cli | `cargo test -p vauchi-cli` |
| vauchi-tui | `cargo test -p vauchi-tui` |
| vauchi-mobile | `cargo test -p vauchi-mobile` |
| vauchi-desktop | `cargo test -p vauchi-desktop` |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `RUST_LOG` | Log level (error/warn/info/debug/trace) |
| `RELAY_URL` | Relay server URL |
| `RELAY_TLS_VERIFIED` | Skip TLS check for relay |

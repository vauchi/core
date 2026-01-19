# Vauchi

A privacy-focused platform for exchanging contact information that stays up-to-date.

## The Problem

When you exchange contact details with someone, that information becomes outdated the moment either of you changes your phone number, email, social media, or address. You end up with stale contacts, and people lose touch.

Worse, social media platforms keep users captive by implicitly threatening them with losing their contacts if they leave. Your relationships become locked inside platforms you may no longer want to use.

## The Solution

Vauchi lets you exchange "living" contact cards. When you update your information, everyone you've shared it with automatically receives the update - securely and privately.

## Key Principles

- **In-Person Exchange** - Contact cards can only be exchanged when physically together (QR code scan)
- **Selective Sharing** - Control which contacts see which fields (work email vs personal)
- **No Messages** - This is not a messenger; it only syncs contact information
- **End-to-End Encrypted** - No server can read your data
- **Decentralized** - Relay servers only pass encrypted blobs; they have zero knowledge

## Repository Structure

This is the main Rust monorepo containing all core components:

```
vauchi-core/     # Core library (cryptography, protocols, data models)
vauchi-relay/    # WebSocket relay server for message forwarding
vauchi-cli/      # Command-line interface for testing
vauchi-tui/      # Terminal UI (ratatui)
vauchi-desktop/  # Desktop app (Tauri + SolidJS)
vauchi-mobile/   # UniFFI bindings for iOS/Android
features/        # Gherkin BDD scenarios
scripts/         # Build and utility scripts
```

## Related Repositories

| Repository | Description |
|------------|-------------|
| [vauchi/android](https://gitlab.com/vauchi/android) | Android app (Kotlin/Compose) - consumes UniFFI bindings |
| [vauchi/ios](https://gitlab.com/vauchi/ios) | iOS app (SwiftUI) - consumes UniFFI bindings |
| [vauchi/website](https://gitlab.com/vauchi/website) | Landing page at vauchi.app |
| [vauchi/docs](https://gitlab.com/vauchi/docs) | User & developer documentation |
| [vauchi/assets](https://gitlab.com/vauchi/assets) | Brand assets, logos, screenshots |
| [vauchi/strategy](https://gitlab.com/vauchi/strategy) | Development, go-live, and community strategy |
| [vauchi/infra](https://gitlab.com/vauchi/infra) | Deployment configs (private) |
| [vauchi/dev-tools](https://gitlab.com/vauchi/dev-tools) | Workspace setup and helper scripts |

## Quick Start

```bash
# Run tests
cargo test --workspace

# Start relay server
cargo run -p vauchi-relay

# CLI commands
cargo run -p vauchi-cli -- init "Alice"
cargo run -p vauchi-cli -- sync
```

## Development

### Prerequisites

- Rust 1.78+ (see `rust-toolchain.toml`)
- For mobile: UniFFI, Swift/Xcode (iOS), Kotlin/Gradle (Android)
- For desktop: Node.js, pnpm, Tauri prerequisites

### Commands

```bash
cargo test                        # All tests
cargo test -p vauchi-core         # Core only
cargo clippy -- -D warnings       # Lint
cargo fmt                         # Format
```

### TDD Workflow

This project uses strict Test-Driven Development:

1. Write failing test (Red)
2. Write minimal code to pass (Green)
3. Refactor
4. Tests trace to `features/*.feature` Gherkin scenarios

See [CLAUDE.md](CLAUDE.md) for detailed rules and project conventions.

## Documentation

- **Architecture**: [vauchi/docs](https://gitlab.com/vauchi/docs) repository
- **API Reference**: Generated from code comments
- **BDD Scenarios**: `features/` directory in this repo

## Mobile Development

The `vauchi-mobile` crate produces UniFFI bindings consumed by:

- **Android**: Clone [vauchi/android](https://gitlab.com/vauchi/android), run `./gradlew build`
- **iOS**: Clone [vauchi/ios](https://gitlab.com/vauchi/ios), open in Xcode

See each platform repo for detailed setup instructions.

## Contributing

1. Read [CLAUDE.md](CLAUDE.md) for project structure and commit rules
2. Check [vauchi/docs](https://gitlab.com/vauchi/docs) for architecture decisions
3. Follow TDD workflow strictly
4. Ensure `cargo test --workspace` passes before submitting

## License

MIT

> [!WARNING]
> **Pre-Alpha Software** - This project is under heavy development and not ready for production use.
> APIs may change without notice. Use at your own risk.

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

### ⚠️ Mandatory Development Rules

**TDD**: Red→Green→Refactor. Test FIRST or delete code and restart.

1. Write failing test (Red)
2. Write minimal code to pass (Green)
3. Refactor
4. Tests trace to `features/*.feature` Gherkin scenarios

**Structure**: `src/` = production code only. `tests/` = tests only. Siblings, not nested.

See [CLAUDE.md](../CLAUDE.md) for additional mandatory rules (crypto, coverage, planning docs).

## Documentation

- **Architecture**: [vauchi/docs](https://gitlab.com/vauchi/docs) repository
- **API Reference**: Generated from code comments
- **BDD Scenarios**: `../features/` (separate repo)

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

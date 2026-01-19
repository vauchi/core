# Vauchi Mobile

UniFFI bindings for Vauchi - exposes the Rust core library to Android and iOS.

## Purpose

This crate wraps `vauchi-core` with a mobile-friendly API via UniFFI. It provides:

- Thread-safe API suitable for mobile UI frameworks
- Simplified data types (records, enums) for cross-language bindings
- On-demand storage connections for SQLite thread safety
- Identity persistence across app sessions

## Building

```bash
# Build for Android
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo build -p vauchi-mobile --target aarch64-linux-android --release

# Build for iOS
rustup target add aarch64-apple-ios x86_64-apple-ios
cargo build -p vauchi-mobile --target aarch64-apple-ios --release

# Generate bindings
cargo run -p vauchi-mobile --bin uniffi-bindgen generate \
    --library target/release/libvauchi_mobile.so \
    --language kotlin --out-dir ./generated
```

## API Overview

```kotlin
// Kotlin usage example
val vauchi = VauchiMobile("path/to/data", "ws://relay.example.com:8080")

// Create identity
vauchi.createIdentity("Alice")

// Add contact fields
vauchi.addField(MobileFieldType.EMAIL, "work", "alice@company.com")
vauchi.addField(MobileFieldType.PHONE, "mobile", "+1-555-1234")

// Exchange contacts
val qrData = vauchi.generateExchangeQr()
// Show QR code to other user...

// Complete exchange from scanned QR
val result = vauchi.completeExchange(scannedQrData)

// List contacts
val contacts = vauchi.listContacts()
```

## Exposed Types

| Type | Description |
|------|-------------|
| `VauchiMobile` | Main interface object |
| `MobileContactCard` | User's contact card |
| `MobileContactField` | Single contact field |
| `MobileContact` | Contact with card and metadata |
| `MobileFieldType` | Email, Phone, Website, Address, Social, Custom |
| `MobileExchangeData` | QR code data for sharing |
| `MobileExchangeResult` | Exchange completion result |
| `MobileSocialNetwork` | Social network info |

## Thread Safety

The API is designed for mobile environments where UI and background threads interact:

- `VauchiMobile` is `Send + Sync`
- Storage connections are created on-demand per operation
- Identity data cached in memory with mutex protection
- Storage encryption key persisted across sessions

## ⚠️ Mandatory Development Rules

**TDD**: Red→Green→Refactor. Test FIRST or delete code and restart.

**Structure**: `src/` = production code only. `tests/` = tests only. Siblings, not nested.

See [CLAUDE.md](../../CLAUDE.md) for additional mandatory rules.

## License

MIT

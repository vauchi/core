# CLAUDE.md - vauchi-mobile

UniFFI bindings for iOS and Android native apps.

## Rules

- Exposes `vauchi-core` to mobile platforms via UniFFI
- Keep binding surface minimal
- Async operations should be properly bridged

## Commands

```bash
cargo build -p vauchi-mobile                # Build bindings
cargo test -p vauchi-mobile                 # Run tests
../scripts/build-bindings.sh                # Generate platform bindings
```

## Integration

- iOS: Bindings consumed by `ios/` SwiftUI app
- Android: Bindings consumed by `android/` Kotlin app

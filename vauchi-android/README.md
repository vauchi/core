# Vauchi Android

Native Android app for privacy-focused contact card exchange.

## Features

- **Contact Card Management**: Create and edit your personal contact card
- **QR Exchange**: Scan or display QR codes to exchange contacts in-person
- **Selective Visibility**: Control which contacts see which fields
- **Background Sync**: Automatic updates via WorkManager (15-min intervals)
- **Encrypted Backup**: Export/import with password-protected encryption

## Tech Stack

- Kotlin + Jetpack Compose (Material Design 3)
- UniFFI bindings to Rust core (`vauchi-mobile`)
- CameraX + ML Kit for QR scanning
- Android KeyStore for secure key storage

## Quick Start

```bash
# Build and install (requires Android SDK)
./gradlew installDebug

# Or open in Android Studio
```

## Requirements

- Android SDK: Compile 35, Min 24, Target 35
- Java 17
- Rust toolchain (for native library)

## Project Structure

```
app/src/main/kotlin/com/vauchi/
├── MainActivity.kt      # Entry point
├── VauchiApp.kt        # Application class
├── ui/screens/          # Compose screens (8 total)
├── data/                # Repository, KeyStore helper
└── viewmodels/          # ViewModel layer
```

## License

MIT

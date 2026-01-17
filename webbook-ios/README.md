# WebBook iOS

Native iOS app for WebBook - privacy-focused contact card exchange.

## Prerequisites

- macOS
- Xcode (download from [App Store](https://apps.apple.com/app/xcode/id497799835) or [developer.apple.com](https://developer.apple.com/download/all/))
- Rust (for building UniFFI bindings)

## Quick Setup

Run the setup script to configure Xcode:

```bash
./scripts/setup-xcode.sh
```

This script will:
1. Verify Xcode installation
2. Accept the license agreement
3. Run first launch setup (installs required components)
4. Download iOS platform and simulators
5. Install Rust iOS targets for UniFFI builds

## Manual Setup

If you prefer manual setup or the script fails:

### 1. Install Xcode

**Option A - App Store:**
- Open App Store, search "Xcode", install

**Option B - Developer Portal (specific versions):**
1. Go to https://developer.apple.com/download/all/
2. Sign in with Apple ID
3. Download Xcode .xip file
4. Extract and move to `/Applications/Xcode.app`

### 2. Accept License Agreement

```bash
sudo xcodebuild -license accept
```

### 3. Install Components

```bash
xcodebuild -runFirstLaunch
```

### 4. Install iOS Platform

```bash
xcodebuild -downloadPlatform iOS
```

Or via Xcode: Settings → Components → iOS

### 5. Install Rust iOS Targets (for UniFFI)

```bash
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
```

## Building

### Command Line

```bash
# Build for simulator
xcodebuild -scheme WebBook -destination 'platform=iOS Simulator,name=iPhone 16' build

# Run tests
xcodebuild -scheme WebBook -destination 'platform=iOS Simulator,name=iPhone 16' test
```

### Xcode

```bash
open WebBook.xcodeproj
```

Then use Cmd+B to build, Cmd+R to run.

## Project Structure

```
webbook-ios/
├── WebBook.xcodeproj      # Xcode project
├── Package.swift          # Swift Package Manager manifest
├── scripts/
│   └── setup-xcode.sh     # Development environment setup
├── WebBook/
│   ├── WebBookApp.swift   # App entry point
│   ├── ContentView.swift  # Root view
│   ├── Views/
│   │   ├── HomeView.swift
│   │   ├── ContactsView.swift
│   │   ├── ContactDetailView.swift
│   │   ├── ExchangeView.swift
│   │   ├── QRScannerView.swift
│   │   ├── SettingsView.swift
│   │   └── SetupView.swift
│   ├── ViewModels/
│   │   └── WebBookViewModel.swift
│   └── Services/
│       ├── WebBookRepository.swift
│       └── KeychainService.swift
└── WebBookTests/
    ├── WebBookRepositoryTests.swift
    └── WebBookViewModelTests.swift
```

## Architecture

The iOS app follows MVVM architecture:

- **Views**: SwiftUI views for UI
- **ViewModels**: Business logic and state management
- **Services**: Data access (WebBookRepository wraps UniFFI bindings)

The app uses `webbook-mobile` UniFFI bindings to call the Rust `webbook-core` library for all cryptographic operations and data storage.

## Troubleshooting

### "No available devices matched the request"
Install iOS simulators:
```bash
xcodebuild -downloadPlatform iOS
```

### "You have not agreed to the Xcode license"
```bash
sudo xcodebuild -license accept
```

### "CoreSimulator.framework not found"
```bash
xcodebuild -runFirstLaunch
```

### Rust targets missing
```bash
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
```

## License

MIT

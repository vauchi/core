# Vauchi Desktop

Cross-platform desktop app for privacy-focused contact card exchange.

## Features

- **Contact Card Management**: Create and edit your personal contact card
- **QR Exchange**: Generate QR codes for contact exchange
- **Selective Visibility**: Control which contacts see which fields
- **Device Linking**: Link multiple devices to sync contacts
- **Encrypted Backup**: Export/import with password-protected encryption

## Tech Stack

- **Backend**: Tauri 2.0 + Rust (`vauchi-core`)
- **Frontend**: Solid.js + TypeScript + Vite
- **Platforms**: macOS, Windows, Linux

## Quick Start

```bash
# Install dependencies
cd ui && npm install

# Development mode
npm run dev
# In another terminal:
cargo tauri dev

# Production build
npm run build && cargo tauri build
```

## Requirements

- Node.js 18+
- Rust toolchain
- Platform-specific build tools (Xcode/MSVC/GCC)

## Project Structure

```
vauchi-desktop/
├── ui/                  # Frontend (Solid.js)
│   ├── src/pages/       # Page components (7 pages)
│   └── src/App.tsx      # Main routing
└── src-tauri/           # Backend (Rust)
    ├── src/commands/    # Tauri IPC handlers
    └── src/lib.rs       # Command registration
```

## License

MIT

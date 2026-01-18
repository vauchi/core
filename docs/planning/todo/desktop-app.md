# Desktop App (Tauri)

**Phase**: 1 | **Complexity**: Medium

## Goal
Cross-platform desktop app (Windows, macOS, Linux).

## Approach
- Tauri framework (Rust backend + web frontend)
- Compile vauchi-core to native (not WASM)
- Simple web UI (HTML/CSS/JS or lightweight framework)

## Requirements
- All functional features
- QR display and webcam scanning
- System tray for background sync
- Platform-appropriate secure storage

## Files to Create
- `vauchi-desktop/` - Tauri project
- Frontend in `vauchi-desktop/src/`
- Tauri commands wrapping vauchi-core

## Dependencies
- Tauri CLI
- Node.js (for frontend build)
- Platform SDKs for building

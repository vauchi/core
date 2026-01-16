# Technology Stack

## Core Library (Shared)

- **Language**: Rust (compiled to native + WebAssembly)
- **Crypto**: ring (audited, production-ready)
- **P2P**: libp2p (rust-libp2p) - planned
- **Storage**: SQLite (encrypted)
- **Serialization**: Protocol Buffers or MessagePack

## Mobile Apps

- **iOS**: Swift UI + Rust FFI (UniFFI)
- **Android**: Kotlin + Rust JNI (UniFFI)

## Desktop Apps

- **Framework**: Tauri (Rust backend + Web frontend)
- **Frontend**: SolidJS or Svelte (lightweight, performant)
- **Platforms**: Windows, macOS, Linux

## Web App (Optional)

- **Runtime**: WebAssembly (shared Rust core)
- **Storage**: IndexedDB (encrypted)
- **Limitations**: No BLE/NFC, QR-only exchange

## Scalability Considerations

### Horizontal Scaling

- No central database to scale
- DHT naturally distributes load
- Relay nodes can be added independently

### Performance Targets

- Contact exchange: < 3 seconds
- Update propagation: < 30 seconds (when online)
- Local operations: < 100ms
- App startup: < 2 seconds

### Data Limits

- Max contact card size: 64KB (encrypted)
- Max contacts per user: 10,000
- Max fields per card: 100

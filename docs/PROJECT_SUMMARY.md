# WebBook Project Summary

## Quick Reference

### What is WebBook?

WebBook is a privacy-focused, decentralized contact card exchange application that allows users to share and update contact information with people they meet in the physical world.

### Core Features

| Feature | Description |
|---------|-------------|
| **Contact Card** | Users maintain a personal contact card with phone, email, social media, address fields |
| **Physical Exchange** | Contact cards can only be exchanged when users are physically together |
| **Visibility Control** | Users control exactly what information each contact can see |
| **Real-time Updates** | Changes automatically sync to contacts who have permission to see them |
| **Multi-device** | Works across iOS, Android, Windows, macOS, and Linux |
| **Decentralized** | Minimal reliance on central servers, E2E encrypted |

### Key Differentiators

1. **No Remote Harvesting**: Contact exchange requires physical proximity verification
2. **Granular Privacy**: Field-level visibility control per contact
3. **Truly Decentralized**: P2P sync with optional volunteer relays
4. **No Messages**: Contact info only, no chat features (prevents spam/abuse)

---

## Document Overview

| Document | Purpose |
|----------|---------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Technical design, data models, protocols |
| [TDD_RULES.md](./TDD_RULES.md) | Test-driven development requirements |
| `features/*.feature` | Gherkin behavior specifications |

---

## Feature Files Summary

| Feature File | Scenarios | Coverage |
|--------------|-----------|----------|
| `identity_management.feature` | 15 | User identity, keys, backup/restore |
| `contact_card_management.feature` | 30 | Adding, editing, removing contact fields |
| `contact_exchange.feature` | 25 | QR, BLE, NFC exchange protocols |
| `visibility_control.feature` | 28 | Privacy rules, groups, propagation |
| `sync_updates.feature` | 30 | P2P sync, conflict resolution |
| `contacts_management.feature` | 32 | Contact list, groups, blocking |
| `security.feature` | 28 | Encryption, keys, attack prevention |
| `device_management.feature` | 25 | Multi-device linking and sync |
| `relay_network.feature` | 22 | Volunteer relay infrastructure |

**Total: ~235 scenarios**

---

## Technology Stack Summary

```
┌─────────────────────────────────────────────────────────┐
│                    PRESENTATION                          │
│  iOS (Swift UI) │ Android (Kotlin) │ Desktop (Tauri)    │
└─────────────────────┬───────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────┐
│                  SHARED CORE (Rust)                      │
│  Crypto │ Storage │ P2P │ Sync │ Business Logic         │
└─────────────────────┬───────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────┐
│                    NETWORK                               │
│  libp2p (DHT/P2P) │ Relay Protocol │ BLE/NFC/Audio      │
└─────────────────────────────────────────────────────────┘
```

---

## Development Phases

### Phase 1: Foundation (Weeks 1-4)
- Core crypto library (Ed25519, X25519, XChaCha20-Poly1305)
- Data models (ContactCard, ContactField, VisibilityRule)
- Local encrypted storage (SQLCipher)
- CLI testing tool

### Phase 2: Exchange Protocol (Weeks 5-8)
- QR code generation/scanning
- Audio proximity verification
- BLE proximity exchange
- X3DH key agreement

### Phase 3: Sync Layer (Weeks 9-12)
- libp2p integration
- DHT-based peer discovery
- CRDT-based conflict resolution
- Update propagation

### Phase 4: Mobile Apps (Weeks 13-18)
- iOS app with Swift UI
- Android app with Kotlin
- Platform-specific features (NFC)

### Phase 5: Desktop Apps (Weeks 19-22)
- Tauri shell
- Cross-platform UI
- Device linking

### Phase 6: Infrastructure (Weeks 23-26)
- Relay node implementation
- Docker deployment
- Network monitoring

### Phase 7: Polish (Weeks 27-30)
- UI/UX refinement
- Accessibility
- Security audit
- Beta release

---

## Getting Started

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development tools
cargo install cargo-tarpaulin  # coverage
cargo install cargo-watch      # auto-reload
```

### Development Workflow

1. **Pick a Gherkin scenario** from `features/`
2. **Write failing tests** following TDD rules
3. **Implement minimal code** to pass tests
4. **Refactor** while keeping tests green
5. **Submit PR** with tests + implementation

### Running Tests

```bash
# Unit tests
cargo test --lib

# Integration tests
cargo test --test integration_*

# Coverage report
cargo tarpaulin --out Html
```

---

## Security Priorities

1. **Private keys** never leave the device unencrypted
2. **All data** encrypted at rest and in transit
3. **Physical proximity** required for exchange
4. **Forward secrecy** via Double Ratchet
5. **No metadata** leakage to relays
6. **Open source** for community audit

---

## Contribution Model

- **Code**: MIT License, open source
- **Relay Nodes**: Volunteer-run, Docker images provided
- **No Freemium**: All features available to all users
- **No Ads**: Privacy-focused, no data monetization

---

## Contact & Links

- **Repository**: [GitHub](https://github.com/your-org/webbook)
- **Documentation**: See `/docs` directory
- **Feature Requests**: GitHub Issues
- **Security Issues**: security@webbook.app (once established)

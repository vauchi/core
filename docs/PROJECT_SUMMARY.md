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
| `social_profile_validation.feature` | 30 | Crowd-sourced profile verification |

**Total: ~265 scenarios**

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

### Phase 1: Foundation ✅
- Crypto via `ring` library (Ed25519, X25519, AES-256-GCM)
- Data models (ContactCard, ContactField, VisibilityRule)
- Local encrypted storage
- CLI testing tool

### Phase 2: Exchange Protocol ✅
- QR code generation/scanning
- X3DH key agreement
- (Future: Audio proximity verification, BLE exchange)

### Phase 3: Sync Layer ✅
- WebSocket relay transport
- Update propagation protocol
- Double Ratchet for forward secrecy
- (Future: libp2p/DHT-based discovery)

### Phase 4: Mobile Apps
- iOS app with Swift UI
- Android app with Kotlin
- Platform-specific features (NFC)

### Phase 5: Desktop Apps
- Tauri shell
- Cross-platform UI
- Device linking

### Phase 6: Infrastructure ✅
- Relay server implementation (webbook-relay)
- (Future: Docker deployment, monitoring)

### Phase 7: CLI Tool ✅
- Full CLI implementation (webbook-cli)
- Identity, card, contact management
- End-to-end exchange via relay

### Phase 8: Complete Integration ✅
- ✅ Bidirectional name exchange
- ✅ Double Ratchet integration (storage, persistence, CLI integration)
- ✅ Card update propagation (encrypted delta sync)
- ✅ Visibility rules enforcement (per-contact field visibility)

### Phase 9: Polish
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
- **Relay Nodes**: Volunteer-run, with Docker, PHP, and Python deployment options
- **No Freemium**: All features available to all users
- **No Ads**: Privacy-focused, no data monetization

---

## Contact & Links

- **Repository**: [GitHub](https://github.com/f-u-f/webbook)
- **Documentation**: See `/docs` directory
- **Feature Requests**: GitHub Issues
- **Security Issues**: mattia.egloff@pm.me

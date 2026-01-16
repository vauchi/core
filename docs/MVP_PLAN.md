# WebBook MVP (Minimum Viable Product) Plan

**Last Updated**: January 2025

## Current Status Summary

### Completed Phases

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Foundation (Crypto, Data Models, Storage) | ✅ Complete |
| 2 | Exchange Protocol (QR, X3DH) | ✅ Complete |
| 3 | Sync Layer (WebSocket, Double Ratchet) | ✅ Complete |
| 6 | Infrastructure (Relay Server) | ✅ Complete |
| 7 | CLI Tool | ✅ Complete |
| 8 | Complete Integration (Visibility Rules, Card Propagation) | ✅ Complete |
| - | Social Network Registry (35+ networks) | ✅ Complete |
| - | Mobile UniFFI Bindings | ✅ Complete |
| - | Relay Persistent Storage (SQLite) | ✅ Complete |
| - | Contact Search | ✅ Complete |

### Test Coverage

| Crate | Tests |
|-------|-------|
| webbook-core | 250 |
| webbook-relay | 20 |
| webbook-cli | 15 |
| webbook-mobile | 10+ |
| **Total** | **~300 tests passing** |

### Feature Specifications

| Feature File | Scenarios | Status |
|--------------|-----------|--------|
| contact_card_management.feature | Core | ✅ Implemented |
| contact_exchange.feature | Core | ✅ Implemented |
| contacts_management.feature | Core | ✅ Implemented |
| identity_management.feature | Core | ✅ Implemented |
| visibility_control.feature | Core | ✅ Implemented |
| sync_updates.feature | Core | ✅ Implemented |
| security.feature | Core | ✅ Implemented |
| device_management.feature | Core | Partial |
| visibility_labels.feature | New | 📝 Specified |
| relay_network.feature | Infra | Partial (federation specified) |
| social_profile_validation.feature | Future | 📝 Specified |
| tor_mode.feature | Privacy (opt-in) | 📝 Specified |
| hidden_contacts.feature | Privacy (opt-in) | 📝 Specified |
| duress_password.feature | Privacy (opt-in) | 📝 Specified |
| **Total** | **~459 scenarios** | |

---

## MVP Definition

The MVP must provide a **complete, usable product** that delivers the core value proposition:

> **Privacy-focused contact card exchange with real-time updates**

### MVP Must-Have Features

| Feature | Description | Status |
|---------|-------------|--------|
| Create identity | Ed25519/X25519 keypair generation | ✅ Done |
| Contact card | Add/edit/remove phone, email, social, address fields | ✅ Done |
| Social networks | 35+ networks with profile URL generation | ✅ Done |
| QR exchange | Generate/scan QR codes for contact exchange | ✅ Done |
| X3DH key agreement | Secure key establishment | ✅ Done |
| Encrypted updates | Double Ratchet forward secrecy | ✅ Done |
| Visibility control | Per-contact field visibility | ✅ Done |
| Update propagation | Card changes sync to contacts | ✅ Done |
| Relay server | WebSocket store-and-forward with SQLite | ✅ Done |
| CLI interface | Full command-line tool | ✅ Done |
| Mobile bindings | UniFFI wrapper for iOS/Android | ✅ Done |
| Identity backup/restore | Encrypted backup with password | ✅ Done |
| Contact search | Search contacts by name | ✅ Done |
| **Mobile app (one platform)** | Android app with full functionality | 🔲 In Progress |

### What's Left for MVP

| Task | Description | Complexity |
|------|-------------|------------|
| **Mobile sync implementation** | Complete `sync()` in webbook-mobile | Medium |
| **Android UI** | Jetpack Compose screens | Medium |
| **QR camera integration** | Scan QR codes on mobile | Medium |
| **Background sync** | WorkManager periodic sync | Low |
| **Polish & testing** | Error handling, UX | Low |

### MVP Nice-to-Have (Post-Launch)

| Feature | Priority | Notes |
|---------|----------|-------|
| iOS app | High | Swift UI, same features |
| Desktop app (Tauri) | High | Cross-platform |
| Visibility labels | High | Group-based visibility (specified) |
| Multi-device sync | Medium | Link devices to one identity |
| Social profile validation | Medium | Crowd-sourced trust |
| BLE/NFC exchange | Medium | Proximity features |
| Tor mode | Low | Opt-in privacy (specified) |
| Hidden contacts | Low | Opt-in privacy (specified) |
| Duress password | Low | Opt-in security (specified) |
| Relay federation | Low | Distributed infrastructure (specified) |

---

## Infrastructure Status

### Relay Server

| Feature | Status |
|---------|--------|
| WebSocket connections | ✅ Done |
| Message store-and-forward | ✅ Done |
| Rate limiting | ✅ Done |
| SQLite persistent storage | ✅ Done |
| 90-day message TTL | ✅ Done |
| Federation protocol | 📝 Specified |
| .onion addresses | 📝 Specified |

### Security

| Aspect | Status |
|--------|--------|
| E2E encryption (AES-256-GCM) | ✅ Done |
| Forward secrecy (Double Ratchet) | ✅ Done |
| Key exchange (X3DH) | ✅ Done |
| Encrypted storage | ✅ Done |
| Threat analysis | ✅ Documented |

---

## MVP Implementation Plan

### Phase MVP-1: Mobile App (Current Focus)

#### MVP-1.1: Complete Mobile Sync ⬅️ **NEXT STEP**

The `webbook-mobile` crate exists but `sync()` is a placeholder.

**Required work:**
1. Implement WebSocket connection to relay in mobile context
2. Send pending updates from local storage
3. Receive and process incoming updates
4. Handle exchange name propagation
5. Update Double Ratchet state after each message

**Files to modify:**
- `webbook-mobile/src/lib.rs` - Complete `sync()` method

#### MVP-1.2: Android App

**Screens needed:**
1. **Welcome/Setup** - Create identity, set display name
2. **My Card** - View/edit own contact card
3. **Contacts List** - List all contacts with search
4. **Contact Detail** - View contact's card, visibility controls
5. **Exchange** - Show/scan QR code
6. **Settings** - Backup/restore, relay URL

**Project structure:**
```
webbook-android/
├── app/
│   ├── src/main/kotlin/
│   │   ├── ui/           # Jetpack Compose screens
│   │   ├── viewmodel/    # ViewModels
│   │   └── repository/   # Rust bridge
│   └── src/main/res/
└── build.gradle.kts
```

#### MVP-1.3: QR Exchange Flow

1. Generate QR with identity + prekey bundle
2. Camera permission and scanning
3. Process scanned QR, initiate X3DH
4. Sync to propagate exchange

#### MVP-1.4: Background Sync

1. WorkManager for periodic sync
2. Sync on app foreground
3. Battery-efficient scheduling

### Phase MVP-2: Polish

- Error handling and user feedback
- Loading and empty states
- Offline mode indicator
- End-to-end testing

---

## Technical Architecture

### Current Crate Structure

```
WebBook/
├── webbook-core/      # 250 tests, feature-complete
│   └── src/
│       ├── api/       # High-level WebBook API
│       ├── crypto/    # Ring-based cryptography
│       ├── identity/  # Identity management
│       ├── contact/   # Contact + visibility
│       ├── contact_card/  # Card data model
│       ├── exchange/  # X3DH, QR codes
│       ├── sync/      # Delta sync
│       ├── network/   # Transport abstraction
│       ├── storage/   # SQLite encrypted storage
│       └── social/    # Social network registry
│
├── webbook-relay/     # 20 tests, production-ready
│   └── src/
│       ├── main.rs    # Server entry
│       ├── storage.rs # SQLite + Memory backends
│       ├── handler.rs # WebSocket handler
│       └── config.rs  # Environment config
│
├── webbook-cli/       # 15 tests, complete
│   └── src/
│       └── commands/  # All CLI commands
│
├── webbook-mobile/    # UniFFI bindings, sync incomplete
│   └── src/
│       └── lib.rs     # Mobile API wrapper
│
└── features/          # 459 Gherkin scenarios
```

### Mobile Data Flow

```
Android UI (Kotlin)
       │
       ▼
   ViewModel
       │
       ▼
   Repository
       │
       ▼
 UniFFI Bindings
       │
       ▼
 webbook-mobile (Rust)
       │
       ▼
  webbook-core (Rust)
       │
       ▼
   SQLite + Relay
```

---

## MVP Success Criteria

### Functional Requirements

- [x] User can create identity and set display name
- [x] User can add/edit/remove contact fields
- [x] User can generate QR code for sharing
- [ ] User can scan QR code to add contact (needs mobile)
- [x] Exchange creates bidirectional contact
- [x] Card updates sync to contacts via relay
- [x] Visibility rules are enforced
- [x] User can backup and restore identity
- [x] User can search contacts

### Non-Functional Requirements

- [ ] App starts in < 2 seconds
- [ ] Exchange completes in < 5 seconds
- [x] Updates propagate (relay stores 90 days)
- [x] Works offline (queues updates)
- [ ] Battery-efficient sync

### Security Requirements

- [x] All data encrypted at rest
- [x] All sync traffic encrypted (E2E)
- [x] Forward secrecy (Double Ratchet)
- [ ] Private keys in Android Keystore (needs mobile)

---

## Post-MVP Roadmap

### v1.1 - iOS App
- Swift UI implementation
- Same features as Android
- TestFlight distribution

### v1.2 - Visibility Labels
- Implement `visibility_labels.feature`
- Group-based visibility control
- Label management UI

### v1.3 - Desktop App
- Tauri framework
- macOS, Windows, Linux
- Device linking

### v1.4 - Enhanced Privacy (Opt-in)
- Tor mode integration
- Hidden contacts
- Duress password

### v1.5 - Multi-Device
- Link multiple devices
- Cross-device sync
- Device management

### v2.0 - Federation
- Relay-to-relay message offloading
- Distributed infrastructure
- .onion relay addresses

---

## Documentation

| Document | Purpose |
|----------|---------|
| `docs/ARCHITECTURE.md` | System design |
| `docs/THREAT_ANALYSIS.md` | Security analysis (25+ threats) |
| `docs/TDD_RULES.md` | Development workflow |
| `docs/PROJECT_SUMMARY.md` | Project overview |
| `CLAUDE.md` | AI assistant instructions |
| `webbook-*/README.md` | Per-crate documentation |
| `webbook-*/STRUCTURE.md` | Code organization |

---

## Conclusion

**The WebBook backend is complete.** All core functionality is implemented, tested, and documented:

- ✅ 300+ tests passing
- ✅ 459 Gherkin scenarios specified
- ✅ Threat analysis complete
- ✅ Relay server with persistent storage
- ✅ Mobile bindings ready

**Remaining for MVP:**
1. Complete mobile sync (placeholder → working)
2. Build Android UI
3. QR camera integration
4. Background sync
5. Polish and test

The mobile app is the only barrier to launching MVP.

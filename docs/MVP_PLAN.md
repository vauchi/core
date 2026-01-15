# WebBook MVP (Minimum Viable Product) Plan

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

### Test Coverage

- **webbook-core**: 250 unit tests passing
- **Integration tests**: 3-user end-to-end tests passing
- **Total scenarios in feature files**: ~275

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
| Relay server | WebSocket store-and-forward | ✅ Done |
| CLI interface | Full command-line tool | ✅ Done |
| **Mobile app (one platform)** | iOS or Android app | 🔲 Not Started |
| Identity backup/restore | Encrypted backup with password | ✅ Done |

### MVP Nice-to-Have (Phase 9+)

These can come after MVP launch:

| Feature | Priority | Notes |
|---------|----------|-------|
| Second mobile platform | High | iOS if Android first, or vice versa |
| Desktop app (Tauri) | High | Cross-platform |
| Social profile validation | Medium | Crowd-sourced trust |
| BLE exchange | Medium | Mobile only, needs hardware |
| NFC exchange | Medium | Mobile only, needs hardware |
| Audio proximity verification | Medium | Enhanced security |
| Multi-device sync | Medium | Link multiple devices to one identity |
| Contact groups | Low | Organizational feature |
| Contact notes | Low | Personal notes |
| Favorites | Low | UI convenience |
| OAuth verification | Low | Future enhancement |
| libp2p/DHT discovery | Low | Full P2P, complex |
| vCard export | Low | Interoperability |

---

## MVP Implementation Plan

### Phase MVP-1: Mobile App Foundation

**Goal**: Create a functional mobile app on ONE platform that demonstrates the full WebBook experience.

**Recommended Platform**: **Android (Kotlin)** - reasons:
- Larger global market share
- More permissive app store policies
- Easier to iterate and deploy
- Direct APK distribution possible

**Alternatively**: **iOS (Swift UI)** if targeting iOS-first market

#### MVP-1.1: Rust Mobile Bindings

Create Rust FFI bindings for mobile platforms.

**Files to create:**
- `webbook-mobile/` - New crate for mobile bindings
- `webbook-mobile/src/lib.rs` - UniFFI bindings
- `webbook-mobile/Cargo.toml` - With UniFFI dependency
- `webbook-mobile/uniffi.toml` - UniFFI configuration

**Deliverable**: Generate Kotlin bindings from webbook-core

```toml
# Cargo.toml
[dependencies]
uniffi = { version = "0.28" }
webbook-core = { path = "../webbook-core" }

[build-dependencies]
uniffi = { version = "0.28", features = ["build"] }
```

#### MVP-1.2: Android App Skeleton

**Files to create:**
- `webbook-android/` - Android project
- `webbook-android/app/src/main/kotlin/` - Kotlin source
- `webbook-android/app/src/main/res/` - Resources

**Screens needed:**
1. **Welcome/Setup** - Create identity, set display name
2. **My Card** - View/edit own contact card
3. **Contacts List** - List all contacts
4. **Contact Detail** - View contact's card
5. **Exchange** - Show/scan QR code
6. **Settings** - Backup/restore, relay URL

#### MVP-1.3: Core Functionality Integration

Wire the Rust core to the Android UI:

| Function | Rust Method | Screen |
|----------|-------------|--------|
| Create identity | `WebBook::create_identity()` | Welcome |
| Get my card | `WebBook::own_card()` | My Card |
| Add field | `WebBook::add_own_field()` | My Card |
| Edit field | `ContactCard::update_field_value()` | My Card |
| Remove field | `WebBook::remove_own_field()` | My Card |
| List contacts | `WebBook::list_contacts()` | Contacts |
| Get contact | `WebBook::get_contact()` | Contact Detail |
| Generate QR | `QrPayloadV2::new()` | Exchange |
| Complete exchange | `ExchangeSession::complete()` | Exchange |
| Sync | `WebBook::sync()` | Background |

#### MVP-1.4: QR Code Exchange

Implement the exchange flow:

1. **Share mode**: Display QR code with identity + prekey
2. **Scan mode**: Camera to scan QR, process exchange
3. **Sync**: Background sync to relay after exchange

**Dependencies:**
- ZXing for QR scanning
- Coil or Glide for image handling

#### MVP-1.5: Background Sync

Implement background synchronization:

1. WorkManager for periodic sync
2. Push notification support (optional for MVP)
3. Sync on app foreground

---

### Phase MVP-2: Polish & Testing

#### MVP-2.1: Error Handling & UX

- Network error messages
- Loading states
- Empty states
- Offline indicator

#### MVP-2.2: End-to-End Testing

- Automated UI tests
- Multi-device manual testing
- Network failure scenarios

#### MVP-2.3: Security Review

- Review of mobile-specific security
- Secure storage of keys
- Screen capture prevention

---

## Technical Architecture for MVP

### Mobile Architecture

```
┌─────────────────────────────────────────┐
│             Android App                  │
│  ┌───────────────────────────────────┐  │
│  │          UI Layer (Kotlin)        │  │
│  │   Jetpack Compose / XML Views     │  │
│  └────────────────┬──────────────────┘  │
│                   │                      │
│  ┌────────────────▼──────────────────┐  │
│  │       ViewModel Layer             │  │
│  │   (StateFlow, LiveData)           │  │
│  └────────────────┬──────────────────┘  │
│                   │                      │
│  ┌────────────────▼──────────────────┐  │
│  │       Repository Layer            │  │
│  │   (Coroutines, suspend)           │  │
│  └────────────────┬──────────────────┘  │
│                   │                      │
│  ┌────────────────▼──────────────────┐  │
│  │         UniFFI Bindings           │  │
│  │   (Auto-generated from Rust)      │  │
│  └────────────────┬──────────────────┘  │
└───────────────────┼─────────────────────┘
                    │
┌───────────────────▼─────────────────────┐
│          webbook-core (Rust)            │
│   Crypto │ Storage │ Sync │ Exchange    │
└─────────────────────────────────────────┘
```

### Data Flow

```
User Action → ViewModel → Repository → UniFFI → Rust Core
                ↓
           UI Update ← StateFlow ← Result
```

---

## MVP Success Criteria

### Functional Requirements

- [ ] User can create identity and set display name
- [ ] User can add/edit/remove contact fields
- [ ] User can generate QR code for sharing
- [ ] User can scan QR code to add contact
- [ ] Exchange creates bidirectional contact
- [ ] Card updates sync to contacts via relay
- [ ] Visibility rules are enforced
- [ ] User can backup and restore identity

### Non-Functional Requirements

- [ ] App starts in < 2 seconds
- [ ] Exchange completes in < 5 seconds
- [ ] Updates propagate in < 30 seconds
- [ ] Works offline (queues updates)
- [ ] Battery-efficient sync

### Security Requirements

- [ ] Private keys stored in Android Keystore
- [ ] All data encrypted at rest
- [ ] All sync traffic encrypted
- [ ] Screen capture blocked on sensitive screens

---

## MVP Timeline Estimates

| Task | Complexity | Notes |
|------|------------|-------|
| MVP-1.1: Rust Mobile Bindings | Medium | UniFFI setup, API design |
| MVP-1.2: Android App Skeleton | Medium | Jetpack Compose setup |
| MVP-1.3: Core Functionality | High | Wire all features |
| MVP-1.4: QR Exchange | Medium | Camera, QR generation |
| MVP-1.5: Background Sync | Medium | WorkManager setup |
| MVP-2.1: Error Handling | Low | UX polish |
| MVP-2.2: E2E Testing | Medium | Test automation |
| MVP-2.3: Security Review | Medium | Security audit |

---

## Post-MVP Roadmap

### v1.1 - iOS App
- Same features as Android
- Swift UI implementation
- TestFlight distribution

### v1.2 - Desktop App
- Tauri framework
- macOS, Windows, Linux
- Device linking to mobile

### v1.3 - Social Validation
- Crowd-sourced profile verification
- Trust levels display
- Validation incentives

### v1.4 - Enhanced Exchange
- BLE proximity exchange
- NFC tap-to-share
- Audio verification

### v1.5 - Multi-Device
- Link multiple devices
- Cross-device sync
- Device management

### v2.0 - Full P2P
- libp2p integration
- DHT-based discovery
- Reduced relay dependency

---

## Files Summary

### Existing (Complete)

| Path | Purpose |
|------|---------|
| `webbook-core/` | Rust core library (250 tests) |
| `webbook-cli/` | CLI tool |
| `webbook-relay/` | WebSocket relay server |
| `features/` | Gherkin specifications (~275 scenarios) |
| `docs/` | Architecture, TDD rules, summaries |

### To Create for MVP

| Path | Purpose |
|------|---------|
| `webbook-mobile/` | UniFFI bindings crate |
| `webbook-android/` | Android app project |
| `docs/MOBILE_SETUP.md` | Mobile development setup |

---

## Getting Started with MVP

```bash
# 1. Set up Android development environment
# Install Android Studio, NDK, Rust android targets

# 2. Add Rust Android targets
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# 3. Create mobile bindings crate
cargo new webbook-mobile --lib

# 4. Configure UniFFI
# Add uniffi dependencies and configure bindings

# 5. Build for Android
cargo build --target aarch64-linux-android --release

# 6. Generate Kotlin bindings
# UniFFI generates bindings automatically

# 7. Create Android project
# Use Android Studio to create new Kotlin project

# 8. Integrate bindings
# Add generated Kotlin files and .so libraries
```

---

## Conclusion

The WebBook core is **MVP-ready**. All essential backend functionality is implemented and tested:

- ✅ Identity management
- ✅ Contact card management
- ✅ Secure exchange protocol
- ✅ Encrypted sync with forward secrecy
- ✅ Visibility control
- ✅ Relay infrastructure
- ✅ CLI for testing

**The only missing piece for MVP is a mobile client.**

Recommended approach:
1. Start with Android (larger market, easier iteration)
2. Use UniFFI for Rust-Kotlin bindings
3. Build minimal UI covering core flows
4. Launch beta for early feedback
5. Then expand to iOS and desktop

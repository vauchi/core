# WebBook Planning

**Last Updated**: January 2025

## Current Status

**MVP Core is functionally complete.** The Android app is fully functional with all core features implemented.

### Quick Status

| Component | Status |
|-----------|--------|
| Core Library (webbook-core) | ✅ Complete - 250 tests |
| Relay Server (webbook-relay) | ✅ Complete - 20 tests |
| CLI Tool (webbook-cli) | ✅ Complete - 15 tests |
| Mobile Bindings (webbook-mobile) | ✅ Complete - 10+ tests |
| Android App | ✅ Complete |
| iOS App | 📝 Planned |

### Test Coverage

| Crate | Tests |
|-------|-------|
| webbook-core | 250 |
| webbook-relay | 20 |
| webbook-cli | 15 |
| webbook-mobile | 10+ |
| **Total** | **~300 tests passing** |

## Planning Documents

### Completed Work

| Document | Description |
|----------|-------------|
| [Phases Completed](./done/phases-completed.md) | All completed development phases |
| [MVP-1: Mobile App](./done/mvp-1-mobile-app.md) | Android app implementation |
| [MVP-2: Polish](./done/mvp-2-polish.md) | Error handling, offline indicator |

### Planned Work

| Document | Description |
|----------|-------------|
| [Camera Scanning](./todo/camera-scanning.md) | Native QR camera integration |
| [Roadmap](./todo/roadmap.md) | Post-MVP feature roadmap |
| [Success Criteria](./todo/success-criteria.md) | MVP success checklist |

## MVP Definition

The MVP delivers the core value proposition:

> **Privacy-focused contact card exchange with real-time updates**

### MVP Features (All Complete)

- ✅ Create identity (Ed25519/X25519 keypair generation)
- ✅ Contact card (Add/edit/remove phone, email, social, address fields)
- ✅ Social networks (35+ networks with profile URL generation)
- ✅ QR exchange (Generate QR codes for contact exchange)
- ✅ X3DH key agreement (Secure key establishment)
- ✅ Encrypted updates (Double Ratchet forward secrecy)
- ✅ Visibility control (Per-contact field visibility)
- ✅ Update propagation (Card changes sync to contacts)
- ✅ Relay server (WebSocket store-and-forward with SQLite)
- ✅ CLI interface (Full command-line tool)
- ✅ Mobile bindings (UniFFI wrapper for iOS/Android)
- ✅ Identity backup/restore (Encrypted backup with password)
- ✅ Contact search (Search contacts by name)
- ✅ Android app (Full functionality)

## Infrastructure

### Relay Server

| Feature | Status |
|---------|--------|
| WebSocket connections | ✅ Done |
| Message store-and-forward | ✅ Done |
| Rate limiting | ✅ Done |
| SQLite persistent storage | ✅ Done |
| 90-day message TTL | ✅ Done |
| Federation protocol | 📝 Specified |

### Security

| Aspect | Status |
|--------|--------|
| E2E encryption (AES-256-GCM) | ✅ Done |
| Forward secrecy (Double Ratchet) | ✅ Done |
| Key exchange (X3DH) | ✅ Done |
| Encrypted storage | ✅ Done |
| Threat analysis | ✅ Documented |

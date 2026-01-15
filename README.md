# WebBook

A privacy-focused platform for exchanging contact information that stays up-to-date.

## The Problem

When you exchange contact details with someone, that information becomes outdated the moment either of you changes your phone number, email, social media, or address. You end up with stale contacts, and people lose touch.

## The Solution

WebBook lets you exchange "living" contact cards. When you update your information, everyone you've shared it with automatically receives the update - securely and privately.

## Key Principles

- **In-Person Exchange** - Contact cards can only be exchanged when physically together (QR code scan)
- **Selective Sharing** - Control which contacts see which fields (work email vs personal)
- **No Messages** - This is not a messenger; it only syncs contact information
- **End-to-End Encrypted** - No server can read your data
- **Decentralized** - Relay servers only pass encrypted blobs; they have zero knowledge

## Project Structure

```
WebBook/
└── webbook-core/     # Core Rust library (complete)
```

### webbook-core

The core library implements all cryptographic protocols and data management. Platform-independent, ready for integration.

See [webbook-core/README.md](webbook-core/README.md) for details.

## Planned Components

- **iOS App** - Native Swift app using webbook-core via FFI
- **Android App** - Native Kotlin app using webbook-core via FFI
- **Desktop Apps** - Cross-platform GUI for macOS, Windows, Linux
- **Relay Server** - Lightweight message relay (voluntary hosting)

## License

MIT

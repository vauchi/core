# MVP-1: Mobile App ✅

## MVP-1.1: Mobile Sync ✅
- WebSocket connection to relay
- Send/receive updates
- Double Ratchet state management

## MVP-1.2: Android App ✅
Jetpack Compose UI with screens:
- Welcome/Setup
- My Card (view/edit)
- Contacts List (with search)
- Contact Detail (visibility controls)
- Exchange (QR generation)
- Settings (backup/restore, relay URL)

## MVP-1.3: QR Exchange ✅
- Generate QR with identity + prekey bundle
- Manual paste of scanned data
- X3DH key agreement
- Camera scanning: see [todo/camera-scanning.md](../todo/camera-scanning.md)

## MVP-1.4: Background Sync ✅
- WorkManager (15 min intervals)
- Battery-efficient (requires network)
- Retry on failure (3 attempts)

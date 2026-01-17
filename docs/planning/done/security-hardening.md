# Security Hardening & Code Maintainability

**Status**: ✅ Complete
**Completed**: January 2026

## Summary

Comprehensive security hardening across iOS/Android and code maintainability improvements through module splitting.

## Security Improvements

### iOS Platform
- **Keychain Protection**: Upgraded to `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`
- **Atomic Updates**: Use `SecItemUpdate` instead of delete+add
- **Biometric Auth**: Required for backup export/import operations
- **Transport Security**: Enforce wss:// only for relay connections
- **Clipboard Security**: Auto-clear after 30 seconds
- **URL Validation**: Block dangerous schemes (javascript, vbscript, data, file)

### Android Platform
- **KeyStore Integration**: Hardware-backed AES-256-GCM when available
- **Secure Key Storage**: Master key in Android KeyStore
- **IV + Ciphertext Format**: Proper authenticated encryption

### Mobile Bindings
- **Certificate Pinning**: `set_pinned_certificate()` API
- **Secure Constructor**: `new_with_secure_key()` for platform key injection
- **Storage Key Export**: Migration support for legacy storage

## Code Maintainability

### webbook-mobile Refactoring
Split `lib.rs` from 1,747 to 891 lines:
- `sync.rs` (468 LOC) - Relay sync operations
- `protocol.rs` (172→55 LOC) - Re-exports from core
- `types.rs` (145 LOC) - UniFFI wrapper types
- `cert_pinning.rs` (85 LOC) - TLS pinning
- `error.rs` (50 LOC) - Error types

### webbook-core/storage Refactoring
Split `mod.rs` from 1,404 lines:
- `contacts.rs` (215 LOC) - Contact operations
- `device.rs` (229 LOC) - Device operations
- `pending.rs` (168 LOC) - Pending updates
- `ratchet.rs` (80 LOC) - Double Ratchet state
- `identity.rs` (62 LOC) - Identity backup
- `error.rs` (42 LOC) - Error types

### Protocol Consolidation
Created `webbook-core/src/network/simple_message.rs`:
- Single source of truth for wire protocol
- Eliminated duplication across mobile/CLI/relay
- Full test coverage for encode/decode

## Commits
- `c6b0001` - Security hardening and mobile module refactoring
- `6855b8f` - Consolidate wire protocol into webbook-core
- `8c0013e` - Split webbook-core storage module into focused submodules

# Security Audit Checklist

This document provides a security checklist for external auditors reviewing WebBook.
It maps security properties to their implementations in the codebase.

## Overview

WebBook is a privacy-focused contact card exchange application. For detailed threat analysis,
see [THREAT_ANALYSIS.md](./THREAT_ANALYSIS.md).

**Key Security Guarantees:**
- End-to-end encryption for all contact data
- Forward secrecy via Double Ratchet protocol
- In-person key exchange only (QR codes)
- No server-side decryption capability
- Cryptographic identity verification

---

## Cryptographic Implementation

### Audit Item 1: Use of Audited Libraries

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| All crypto uses `ring` crate (audited) | ✅ Verified | `webbook-core/Cargo.toml` |
| No custom cryptographic algorithms | ✅ Verified | All crypto in `src/crypto/` uses ring |
| Random number generation uses SystemRandom | ✅ Verified | `src/crypto/mod.rs` |

**Files to Review:**
- `webbook-core/src/crypto/mod.rs` - Core crypto exports
- `webbook-core/src/crypto/encryption.rs` - AES-256-GCM implementation
- `webbook-core/src/crypto/signing.rs` - Ed25519 signatures
- `webbook-core/src/crypto/kdf.rs` - HKDF-SHA256 key derivation

### Audit Item 2: Key Zeroing

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Keys zeroed on drop (zeroize crate) | ✅ Verified | `SymmetricKey`, `SigningKeyPair` |
| Sensitive data has `Zeroize` derive | ✅ Verified | All key types |
| No copies of keys left in memory | ✅ Verified | Keys returned by reference |

**Files to Review:**
- `webbook-core/src/crypto/mod.rs:SymmetricKey` - Implements `Zeroize`
- `webbook-core/src/identity/mod.rs:Identity` - Master seed zeroized on drop

### Audit Item 3: Algorithm Parameters

| Algorithm | Usage | Parameters |
|-----------|-------|------------|
| AES-256-GCM | Data encryption | 256-bit key, 96-bit nonce, 128-bit tag |
| Ed25519 | Signatures | Per RFC 8032 |
| X25519 | Key agreement | Per RFC 7748 |
| HKDF-SHA256 | Key derivation | Per RFC 5869 |
| PBKDF2-HMAC-SHA256 | Password derivation | 100,000 iterations |

---

## Key Management

### Audit Item 4: Master Seed Protection

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Master seed never stored in plaintext | ✅ Verified | Always encrypted for backup |
| Seed derived keys use domain separation | ✅ Verified | HKDF with unique info strings |
| Backup encryption uses PBKDF2 | ✅ Verified | 100k iterations |

**Files to Review:**
- `webbook-core/src/identity/mod.rs:export_backup()` - PBKDF2 derivation
- `webbook-core/src/identity/mod.rs:from_seed()` - Key derivation with HKDF
- `webbook-core/src/crypto/kdf.rs` - HKDF implementation

### Audit Item 5: Password Strength Enforcement

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Minimum 8 characters | ✅ Verified | `src/identity/password.rs` |
| zxcvbn score >= 3 required | ✅ Verified | Added in Phase 2 |
| Entropy-based validation | ✅ Verified | zxcvbn crate v3 |

**Files to Review:**
- `webbook-core/src/identity/password.rs` - Password validation with zxcvbn
- Tests: `webbook-core/tests/identity_tests.rs` - Password strength tests

### Audit Item 6: Platform Secure Storage

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| SecureStorage trait defined | ✅ Verified | `src/storage/secure.rs` |
| Platform keyring support | ✅ Verified | `keyring` crate (feature flag) |
| Encrypted file fallback | ✅ Verified | `FileKeyStorage` |

**Files to Review:**
- `webbook-core/src/storage/secure.rs` - SecureStorage trait and implementations
- `webbook-core/Cargo.toml` - `secure-storage` feature flag

---

## Forward Secrecy

### Audit Item 7: Double Ratchet Protocol

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Double Ratchet implemented | ✅ Verified | `src/crypto/ratchet.rs` |
| Message keys deleted after use | ✅ Verified | Keys derived per-message |
| Ratchet state persisted for recovery | ✅ Verified | `storage.save_ratchet_state()` |

**Files to Review:**
- `webbook-core/src/crypto/ratchet.rs` - Double Ratchet implementation
- `webbook-core/src/crypto/chain.rs` - Chain key ratcheting
- Tests: `webbook-core/tests/crypto_tests.rs` - Ratchet tests

### Audit Item 8: X3DH Key Agreement

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| X3DH implemented correctly | ✅ Verified | `src/exchange/x3dh.rs` |
| Ephemeral keys generated per exchange | ✅ Verified | New keypair each time |
| Shared secret properly derived | ✅ Verified | HKDF with concatenated secrets |

**Files to Review:**
- `webbook-core/src/exchange/x3dh.rs` - X3DH implementation
- `webbook-core/src/exchange/session.rs` - Session establishment

---

## Protocol Security

### Audit Item 9: QR Code Security

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| QR codes contain signed data | ✅ Verified | `src/exchange/qr.rs` |
| QR payloads expire | ✅ Verified | 10-minute default expiry |
| QR data is versioned | ✅ Verified | Version byte in payload |

**Files to Review:**
- `webbook-core/src/exchange/qr.rs` - QR code generation/parsing
- `webbook-core/src/exchange/mod.rs` - Exchange protocol

### Audit Item 10: Device Linking

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Device linking requires signature verification | ✅ Verified | `src/identity/device.rs` |
| Device registry is signed | ✅ Verified | Ed25519 signature |
| Maximum device limit enforced | ✅ Verified | `MAX_DEVICES = 10` |

**Files to Review:**
- `webbook-core/src/identity/device.rs` - Device management
- Tests: `webbook-core/tests/identity_tests.rs` - Device tests

### Audit Item 11: Relay Communication

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| All relay communication is E2E encrypted | ✅ Verified | `src/network/protocol.rs` |
| TLS for transport layer | ✅ Verified | WebSocket over TLS |
| Relay cannot decrypt content | ✅ Verified | No key access |

**Files to Review:**
- `webbook-core/src/network/protocol.rs` - Protocol messages
- `webbook-core/src/network/relay_client.rs` - Relay client
- `webbook-relay/src/` - Relay server (verify no decryption)

---

## Storage Security

### Audit Item 12: Local Database

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| SQLite with application-level encryption | ✅ Verified | `src/storage/mod.rs` |
| Contact cards encrypted per-contact | ✅ Verified | AES-GCM encryption |
| Shared secrets never logged | ✅ Verified | No logging of keys |

**Files to Review:**
- `webbook-core/src/storage/mod.rs` - Storage implementation
- `webbook-core/src/contact/mod.rs` - Contact handling

### Audit Item 13: Backup Security

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Backups are password-encrypted | ✅ Verified | `export_backup()` |
| Strong password required | ✅ Verified | zxcvbn score >= 3 |
| Backup format is versioned | ✅ Verified | For future compatibility |

**Files to Review:**
- `webbook-core/src/identity/backup.rs` - Backup format
- `webbook-core/src/identity/mod.rs` - export/import backup

---

## Testing Coverage

### Security-Focused Tests

| Test Category | Location | Status |
|---------------|----------|--------|
| Crypto unit tests | `tests/crypto_tests.rs` | ✅ Passing |
| Fuzz tests (1000+ cases) | `tests/fuzz_tests.rs` | ✅ Passing |
| Property-based tests | `tests/property_tests.rs` | ✅ Passing |
| Concurrency tests | `tests/concurrency_tests.rs` | ✅ Passing |
| Protocol compatibility | `tests/protocol_compatibility_tests.rs` | ✅ Passing |
| Password validation | `tests/identity_tests.rs` | ✅ Passing |
| Secure storage | `src/storage/secure.rs` (mod tests) | ✅ Passing |

### Test Commands

```bash
# Run all security tests
cargo test -p webbook-core

# Run with secure-storage feature
cargo test -p webbook-core --features secure-storage

# Run fuzz tests
cargo test -p webbook-core fuzz

# Run property tests
cargo test -p webbook-core property

# Run benchmarks
cargo bench -p webbook-core
```

---

## Dependency Audit

### Critical Dependencies

| Crate | Version | Purpose | Audit Status |
|-------|---------|---------|--------------|
| ring | 0.17 | Cryptography | ✅ Audited by Google |
| x25519-dalek | 2.0 | X25519 | ✅ Audited (dalek-cryptography) |
| zeroize | 1.8 | Memory zeroing | ✅ Widely audited |
| zxcvbn | 3.1 | Password strength | ✅ Port of Dropbox zxcvbn |
| keyring | 3.6 | Platform keychains | Review recommended |

### Checking Dependencies

```bash
# Check for known vulnerabilities
cargo audit

# Check for outdated dependencies
cargo outdated
```

---

## Recommendations for Auditors

### Priority Areas

1. **Cryptographic Implementation** - Verify correct use of `ring` primitives
2. **Key Derivation** - Verify HKDF domain separation and PBKDF2 parameters
3. **Double Ratchet** - Verify forward secrecy properties
4. **QR Exchange** - Verify signature validation and expiry checks
5. **Password Validation** - Verify zxcvbn integration is correct

### Known Limitations

1. Platform keyring tests require desktop session (marked `#[ignore]`)
2. Some doc tests are ignored (network-dependent)
3. Fuzz tests use bounded iterations (not exhaustive)

### Security Contacts

- Report vulnerabilities: security@webbook.app (planned)
- GitHub security advisories: github.com/webbook/webbook-core/security

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-01 | Initial security audit checklist |

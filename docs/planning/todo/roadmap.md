# Roadmap

## Phase 1: Cross-Platform Apps

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Multi-device sync | High | High (core arch) | ✅ Done |
| iOS app | Medium | Low | Todo |
| Desktop app (Tauri) | Medium | Low | Todo |

### Multi-Device Sync (Completed)
- Device module with DeviceInfo and DeviceRegistry
- Device linking protocol (QR-based)
- Device-to-device contact sync
- Sync orchestration with version vectors
- Device revocation certificates
- CLI device management commands
- Architecture docs and threat analysis (8 scenarios)

## Phase 2: Security & Quality

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Security audit | Medium | High (could find flaws) | ✅ Done |
| Password strength (zxcvbn) | Low | Low | ✅ Done |
| Keys in secure storage | Medium | Medium | ✅ Done |
| Security audit checklist | Low | Low | ✅ Done |
| UI/UX review | Low | Low | Todo |
| Performance tuning | Low | Low | Todo |

### Phase 2 Implementation Details

**Password Strength Enforcement:**
- Added zxcvbn dependency for entropy-based password validation
- Requires minimum score of 3 (out of 4) for backup passwords
- `webbook-core/src/identity/password.rs` - validation logic
- Tests in `webbook-core/tests/identity_tests.rs`

**Secure Storage:**
- Added `keyring` crate (optional) for platform keychain access
- `SecureStorage` trait with platform implementations
- `PlatformKeyring` - uses OS keychain (macOS/Linux/Windows)
- `FileKeyStorage` - encrypted file fallback
- `webbook-core/src/storage/secure.rs`

**Security Audit Checklist:**
- `docs/SECURITY_AUDIT.md` - comprehensive checklist for external auditors
- Maps security properties to code implementations
- Dependency audit status

### Testing (Completed)

See `docs/development/testing.md` for full strategy.

| Task | Value | Complexity | Status |
|------|-------|------------|--------|
| Fuzz testing (parsers) | High | Medium | ✅ Done |
| Concurrency tests (storage) | High | Medium | ✅ Done |
| Protocol compatibility tests | High | Low | ✅ Done |
| Migration tests (database) | High | Low | ✅ Done |
| Snapshot tests (serialization) | Medium | Low | ✅ Done |
| Performance benchmarks | Medium | Low | ✅ Done |
| Property-based tests | High | Medium | ✅ Done |

**Test Files:**
- `webbook-core/tests/fuzz_tests.rs` - 1000+ fuzz test cases
- `webbook-core/tests/concurrency_tests.rs` - Thread safety tests
- `webbook-core/tests/protocol_compatibility_tests.rs` - Golden fixtures
- `webbook-core/tests/migration_tests.rs` - Schema verification
- `webbook-core/tests/snapshot_tests.rs` - Serialization snapshots
- `webbook-core/benches/crypto_benchmarks.rs` - Criterion benchmarks
- `webbook-core/tests/property_tests.rs` - Proptest properties

## Phase 3: Launch

### Core Features

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Contact recovery | High | High | ✅ Done |
| SecureStorage integration | Medium | Low | ✅ Done |
| Full iOS app integration | Medium | Medium | Todo |
| Full Desktop app integration | Medium | Medium | Todo |

### Privacy & Security

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Tor mode | Medium | Medium | Todo |
| Hidden contacts | Low | Low | Todo |
| Duress password | Low | Low | Todo |
| Random jitter for sync timing | Low | Low | Todo |

### Exchange Methods

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| BLE/NFC exchange | Medium | Medium | Todo |
| Audio proximity verification | Medium | Medium | Todo |

### Infrastructure

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Relay deployment | Medium | Medium | Todo |
| Docker deployment | Low | Low | Todo |
| Relay federation | High | High | Todo |
| Monitoring and health checks | Low | Low | Todo |
| libp2p/DHT discovery | High | High | Todo |

### Distribution

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Release signing | Low | Low | Todo |
| ProGuard/R8 | Low | Low | Todo |
| App store listings | Low | Low | Todo |
| Privacy policy | Low | Low | Todo |
| Desktop distribution | Low | Low | Todo |

### Polish

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Visibility labels | Low | Low | Todo |
| Social profile validation | Medium | Low | Todo |
| UI/UX review | Low | Low | Todo |
| Performance tuning | Low | Low | Todo |

---

### Feature Details

**Contact Recovery:**
- Social vouching system (K-of-N contacts vouch for identity)
- Spec: `docs/planning/done/P3-contact-recovery.md`
- Threat analysis: `docs/THREAT_ANALYSIS.md` (T9.x threats)

**SecureStorage Integration:**
- Use `PlatformKeyring` in CLI/TUI/Desktop apps
- Store encryption keys in OS keychain instead of files
- Trait ready: `webbook-core/src/storage/secure.rs`

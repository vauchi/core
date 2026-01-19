# Roadmap

## Phase 1: Cross-Platform Apps

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Multi-device sync | High | High (core arch) | ✅ Done |
| iOS app | Medium | Low | ✅ Done (85%) |
| Desktop app (Tauri) | Medium | Low | ✅ Done |
| TUI app | Low | Low | ✅ Done |

### iOS App (Completed - Jan 2026)
- 7 SwiftUI screens with full feature set
- UniFFI bindings integration
- Keychain secure storage with biometric auth
- Background sync via BGTaskScheduler
- See `docs/planning/done/ios-app.md`

### Desktop App (Completed)
- Tauri 2.0 + Solid.js frontend
- Cross-platform (macOS, Windows, Linux)
- Full feature parity with mobile

### TUI App (Completed)
- Ratatui terminal UI
- 12 screens with keyboard navigation
- Direct vauchi-core integration

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
| Security hardening (iOS/Android) | Medium | Medium | ✅ Done |
| Code maintainability refactor | Medium | Low | ✅ Done |
| UI/UX review | Low | Low | Todo |
| Performance tuning | Low | Low | Todo |

### Security Hardening (Completed - Jan 2026)
- iOS: Keychain protection upgrade, biometric auth, wss:// enforcement
- Android: KeyStore with hardware-backed encryption
- Mobile: Certificate pinning, secure key constructor
- Clipboard auto-clear (30 seconds)
- See `docs/planning/done/security-hardening.md`

### Code Maintainability (Completed - Jan 2026)
- Split vauchi-mobile/lib.rs (1,747→891 lines)
- Split vauchi-core/storage (1,404 lines into 6 modules)
- Consolidated wire protocol in vauchi-core

### Phase 2 Implementation Details

**Password Strength Enforcement:**
- Added zxcvbn dependency for entropy-based password validation
- Requires minimum score of 3 (out of 4) for backup passwords
- `vauchi-core/src/identity/password.rs` - validation logic
- Tests in `vauchi-core/tests/identity_tests.rs`

**Secure Storage:**
- Added `keyring` crate (optional) for platform keychain access
- `SecureStorage` trait with platform implementations
- `PlatformKeyring` - uses OS keychain (macOS/Linux/Windows)
- `FileKeyStorage` - encrypted file fallback
- `vauchi-core/src/storage/secure.rs`

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
- `vauchi-core/tests/fuzz_tests.rs` - 1000+ fuzz test cases
- `vauchi-core/tests/concurrency_tests.rs` - Thread safety tests
- `vauchi-core/tests/protocol_compatibility_tests.rs` - Golden fixtures
- `vauchi-core/tests/migration_tests.rs` - Schema verification
- `vauchi-core/tests/snapshot_tests.rs` - Serialization snapshots
- `vauchi-core/benches/crypto_benchmarks.rs` - Criterion benchmarks
- `vauchi-core/tests/property_tests.rs` - Proptest properties

## Phase 3: Launch

### Core Features

| Task | Complexity | Risk | Status |
|------|------------|------|--------|
| Contact recovery | High | High | ✅ Done |
| SecureStorage integration | Medium | Low | ✅ Done |
| Full iOS app integration | Medium | Medium | ✅ Done |
| Full Desktop app integration | Medium | Medium | ✅ Done |

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
| Relay deployment | Medium | Medium | ✅ Config Ready |
| Docker deployment | Low | Low | ✅ Done |
| Relay federation | High | High | Todo |
| Monitoring and health checks | Low | Low | Todo |
| libp2p/DHT discovery | High | High | Todo |

**Relay Deployment Notes (2026-01-19)**:
- Kamal config created at `infra/hosts/bold-hopper/deploy.yml`
- DNS already configured: relay.vauchi.app → 87.106.25.46
- Server provisioned via Ansible (kamal-ready role)
- Deploy with: `cd infra/hosts/bold-hopper && kamal deploy`

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
| Visibility labels | Low | Low | ✅ Done |
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
- Trait ready: `vauchi-core/src/storage/secure.rs`

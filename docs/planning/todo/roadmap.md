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
| Security audit | Medium | High (could find flaws) | Todo |
| Keys in secure storage | Medium | Medium | Todo |
| UI/UX review | Low | Low | Todo |
| Performance tuning | Low | Low | Todo |

### Testing (Fail Fast)

See `docs/development/testing.md` for full strategy.

| Task | Value | Complexity | Status |
|------|-------|------------|--------|
| Fuzz testing (parsers) | High | Medium | Todo |
| Concurrency tests (storage) | High | Medium | Todo |
| Protocol compatibility tests | High | Low | Todo |
| Migration tests (database) | High | Low | Todo |
| Snapshot tests (serialization) | Medium | Low | Todo |
| Performance benchmarks | Medium | Low | Todo |

## Phase 3: Launch

| Task | Complexity | Status |
|------|------------|--------|
| Release signing | Low | Todo |
| ProGuard/R8 | Low | Todo |
| Relay deployment | Medium | Todo |
| App store listings | Low | Todo |
| Privacy policy | Low | Todo |
| Desktop distribution | Low | Todo |

---

## Post-Launch

| Feature | Complexity |
|---------|------------|
| Visibility labels | Low |
| BLE/NFC exchange | Medium |
| Social profile validation | Medium |
| Docker deployment | Low |
| Monitoring and health checks | Low |
| Tor mode | Medium |
| Hidden contacts | Low |
| Duress password | Low |
| Relay federation | High |
| libp2p/DHT discovery | High |
| Audio proximity verification | Medium |

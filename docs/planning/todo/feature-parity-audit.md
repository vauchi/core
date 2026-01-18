# Feature Parity Audit & Action Plans

**Date**: January 2026
**Scope**: CLI, TUI, Desktop, Android, iOS + Relay Server

---

## Feature Comparison Matrix

Legend: ✅ Complete | ⚠️ Partial | ❌ Missing | 📋 UI Only (delegates to CLI)

### Core Identity Features

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Create identity | ✅ | ✅ | ✅ | ✅ | ✅ |
| View identity info | ✅ | ✅ | ✅ | ✅ | ✅ |
| Edit display name | ❌ | ❌ | ❌ | ❌ | ❌ |

### Contact Card Management

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| View own card | ✅ | ✅ | ✅ | ✅ | ✅ |
| Add field | ✅ | ✅ | ✅ | ✅ | ✅ |
| Edit field value | ✅ | ❌ | ❌ | ✅ | ❌ |
| Remove field | ✅ | ✅ | ❌ | ✅ | ✅ |
| Field type icons | N/A | ✅ | ✅ | ✅ | ✅ |

### Contact Exchange

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Generate QR | ✅ | ✅ | ✅ | ✅ | ✅ |
| QR expiration timer | ✅ | ❌ | ❌ | ✅ | ✅ |
| Camera QR scan | N/A | N/A | ❌ | ✅ | ✅ |
| Manual data input | ✅ | ❌ | ✅ | ✅ | ❌ |
| Complete exchange | ✅ | ✅ | ✅ | ✅ | ✅ |

### Contact Management

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| List contacts | ✅ | ✅ | ✅ | ✅ | ✅ |
| View contact detail | ✅ | ✅ | ✅ | ✅ | ✅ |
| Search contacts | ✅ | ❌ | ❌ | ✅ | ✅ |
| Delete contact | ✅ | ✅ | ✅ | ✅ | ✅ |
| Verify contact | ✅ | ✅ | ❌ | ❌ | ✅ |

### Field Actions (Open in External App)

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Phone call | ✅ | ✅ | ✅ | ✅ | ✅ |
| SMS | ✅ | ✅ | ✅ | ❌ | ✅ |
| Email | ✅ | ✅ | ✅ | ✅ | ✅ |
| Website | ✅ | ✅ | ✅ | ✅ | ✅ |
| Address/Maps | ✅ | ✅ | ✅ | ✅ | ✅ |
| Social profiles | ✅ | ✅ | ✅ | ✅ | ✅ |

### Visibility Control

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Hide field from contact | ✅ | ✅ | ✅ | ✅ | ✅ |
| Show field to contact | ✅ | ✅ | ✅ | ✅ | ✅ |
| View visibility rules | ✅ | ✅ | ✅ | ✅ | ✅ |
| Bulk visibility (all contacts) | ❌ | ❌ | ✅ | ❌ | ❌ |

### Sync & Relay

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Manual sync | ✅ | ⚠️ | ❌ | ✅ | ✅ |
| Sync status display | ✅ | ⚠️ | ❌ | ✅ | ✅ |
| Configure relay URL | ✅ | ❌ | ❌ | ✅ | ✅ |
| Background sync | N/A | N/A | ❌ | ✅ | ⚠️ |
| Offline indicator | N/A | ❌ | ❌ | ✅ | ✅ |

### Backup & Restore

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Export backup | ✅ | ✅ | ✅ | ✅ | ✅ |
| Import backup | ✅ | ✅ | ❌ | ✅ | ✅ |
| Password strength check | ❌ | ❌ | ✅ | ❌ | ✅ |
| Biometric auth for backup | N/A | N/A | ❌ | ❌ | ✅ |

### Device Management

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| View current device | ✅ | ✅ | ✅ | ✅ | ✅ |
| List linked devices | ⚠️ | ⚠️ | ✅ | ⚠️ | ❌ |
| Generate link QR | ⚠️ | ⚠️ | ✅ | ✅ | ❌ |
| Join via link | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Revoke device | ⚠️ | ❌ | ❌ | ❌ | ❌ |

### Recovery

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Create recovery claim | ✅ | 📋 | ✅ | 📋 | ❌ |
| Vouch for contact | ✅ | 📋 | ✅ | 📋 | ❌ |
| Add voucher | ✅ | ❌ | ❌ | ❌ | ❌ |
| View recovery status | ✅ | 📋 | ❌ | 📋 | ❌ |
| Verify recovery proof | ✅ | ❌ | ❌ | ❌ | ❌ |

### Social Networks

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| List networks | ✅ | ❌ | ❌ | ✅ | ✅ |
| Search networks | ✅ | ❌ | ❌ | ✅ | ✅ |
| Generate profile URL | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## Critical Gaps Summary

### HIGH IMPACT - Missing Across Multiple Frontends

1. **Edit display name** - Missing in ALL frontends
2. **Edit field value** - Only CLI and Android have it
3. **Remove field from Desktop** - Can add but not delete
4. **Search contacts** - Missing in TUI and Desktop
5. **Contact verification** - Missing in Desktop and Android
6. **Sync UI in Desktop** - No sync trigger or status
7. **Import backup in Desktop** - Backend exists, no UI
8. **Device management** - Incomplete everywhere except partial CLI
9. **Recovery workflow** - Only CLI has full implementation

### MEDIUM IMPACT - UX Improvements Needed

10. **QR expiration timer** - Missing in TUI and Desktop
11. **Manual QR data input** - Missing in TUI and iOS
12. **Bulk visibility controls** - Only Desktop has it
13. **Password strength** - Missing in CLI, TUI, Android
14. **Relay URL config** - Missing in TUI and Desktop

---

## Relay Server Critical Issues

| Issue | Severity | Impact |
|-------|----------|--------|
| Connection limit not enforced | HIGH | Resource exhaustion DoS |
| No TLS support | HIGH | Plaintext transmission |
| SQLite single-threaded | MEDIUM-HIGH | Throughput bottleneck |
| Rate limiter memory leak | MEDIUM | Memory exhaustion |
| No input validation | MEDIUM | Potential crashes |
| Metrics not recorded | LOW | No observability |

---

## Action Plans

### Plan 1: Edit Display Name (All Frontends)
**Impact**: HIGH | **Effort**: LOW
**Files to modify**:
- `vauchi-core/src/api.rs` - Add `update_display_name()` method
- `vauchi-mobile/src/lib.rs` - Add UniFFI binding
- CLI: `vauchi-cli/src/commands/init.rs` - Add `name edit` subcommand
- TUI: `vauchi-tui/src/ui/home.rs` - Add edit dialog
- Desktop: `vauchi-desktop/ui/src/pages/Settings.tsx` - Make name editable
- Android: `MainActivity.kt` - Add edit button in Home
- iOS: `SettingsView.swift` - Make name editable

### Plan 2: Field Edit/Remove for Desktop
**Impact**: HIGH | **Effort**: LOW
**Files to modify**:
- `vauchi-desktop/ui/src/pages/Home.tsx` - Add edit/delete buttons to field rows
- `vauchi-desktop/src-tauri/src/card.rs` - Already has `remove_field`, just wire UI

### Plan 3: Contact Search (TUI + Desktop)
**Impact**: MEDIUM | **Effort**: LOW
**Files to modify**:
- TUI: `vauchi-tui/src/ui/contacts.rs` - Add search input field
- TUI: `vauchi-tui/src/handlers/input.rs` - Handle search input
- Desktop: `vauchi-desktop/ui/src/pages/Contacts.tsx` - Add search bar

### Plan 4: Contact Verification (Desktop + Android)
**Impact**: MEDIUM | **Effort**: LOW
**Files to modify**:
- Desktop: `vauchi-desktop/ui/src/pages/Contacts.tsx` - Add verify button
- Desktop: `vauchi-desktop/src-tauri/src/contacts.rs` - Add verify command
- Android: `ContactDetailScreen.kt` - Add verify button and ViewModel method

### Plan 5: Sync UI for Desktop
**Impact**: HIGH | **Effort**: MEDIUM
**Files to modify**:
- `vauchi-desktop/src-tauri/src/lib.rs` - Add sync command
- `vauchi-desktop/ui/src/pages/Settings.tsx` - Add sync button and status
- `vauchi-desktop/ui/src/pages/Home.tsx` - Add sync status indicator

### Plan 6: Import Backup UI for Desktop
**Impact**: MEDIUM | **Effort**: LOW
**Files to modify**:
- `vauchi-desktop/ui/src/pages/Settings.tsx` - Add import section with file picker
- Backend `import_backup` command already exists

### Plan 7: QR Expiration Timer (TUI + Desktop)
**Impact**: LOW | **Effort**: LOW
**Files to modify**:
- TUI: `vauchi-tui/src/ui/exchange.rs` - Add countdown display
- Desktop: `vauchi-desktop/ui/src/pages/Exchange.tsx` - Add timer component

### Plan 8: Device Management Completion
**Impact**: HIGH | **Effort**: HIGH
**Scope**: Complete device linking across all frontends
**Files to modify**:
- Core: Verify `vauchi-core/src/exchange/device_link.rs` is complete
- Mobile: Add UniFFI bindings for device operations
- All frontends: Add device list, link, join, revoke UIs
**Recommendation**: Start with CLI completion, then propagate to mobile

### Plan 9: Recovery UI for Mobile
**Impact**: HIGH | **Effort**: MEDIUM
**Files to modify**:
- Android: Implement actual claim/vouch logic in RecoveryScreen
- iOS: Add recovery views (currently missing)
- Mobile bindings: Verify recovery UniFFI exports

### Plan 10: Relay Server Hardening
**Impact**: CRITICAL | **Effort**: MEDIUM
**Priority fixes**:
1. `vauchi-relay/src/main.rs` - Enforce max_connections
2. Add TLS support or enforce proxy docs
3. `vauchi-relay/src/rate_limit.rs` - Add bucket cleanup
4. `vauchi-relay/src/storage/sqlite.rs` - Enable WAL mode

---

## Implementation Priority

### Phase 1: Quick Wins (1-2 days each)
1. Plan 2: Field Edit/Remove for Desktop
2. Plan 3: Contact Search for TUI + Desktop
3. Plan 6: Import Backup UI for Desktop
4. Plan 7: QR Expiration Timer

### Phase 2: Core Features (3-5 days each)
5. Plan 1: Edit Display Name
6. Plan 4: Contact Verification
7. Plan 5: Sync UI for Desktop

### Phase 3: Complex Features (1-2 weeks each)
8. Plan 8: Device Management
9. Plan 9: Recovery UI for Mobile

### Phase 4: Infrastructure (1 week)
10. Plan 10: Relay Server Hardening

---

## Feature Parity Checklist

After completing all plans, every frontend should have:
- [ ] Create/view/edit identity
- [ ] Add/edit/remove card fields
- [ ] Generate and scan QR (or manual input)
- [ ] List/search/view/delete contacts
- [ ] Verify contacts
- [ ] Field visibility per contact
- [ ] Manual sync with status
- [ ] Export/import backup
- [ ] Device listing and linking
- [ ] Recovery claim/vouch workflow

# P8: Feature-Complete Upgrade Plan

**Status**: In Progress
**Priority**: P0 (Required for production)
**Created**: 2026-01-19

## Overview

This document tracks the work required to make all Vauchi frontends feature-complete and production-ready. Updated after implementing Desktop sync with real relay communication.

---

## Completed Work (2026-01-19)

### Infrastructure

| Task | Status | Details |
|------|--------|---------|
| Kamal deployment config | **DONE** | `infra/hosts/bold-hopper/deploy.yml` created |
| Relay DNS | **DONE** | relay.vauchi.app → 87.106.25.46 |
| Server provisioning | **DONE** | Ansible playbooks ready |

### Frontend Production URLs

| Platform | Old Default | New Default | Status |
|----------|-------------|-------------|--------|
| Android | ws://localhost:8080 | wss://relay.vauchi.app | **DONE** |
| iOS | wss://relay.vauchi.app | wss://relay.vauchi.app | Already correct |
| Desktop | wss://relay.vauchi.app | wss://relay.vauchi.app | Already correct |
| CLI | ws://localhost:8080 | wss://relay.vauchi.app | **DONE** |
| TUI | wss://relay.vauchi.app | wss://relay.vauchi.app | Already correct |

### Desktop Sync Implementation

| Task | Status | Details |
|------|--------|---------|
| WebSocket relay connection | **DONE** | Using tungstenite with TLS |
| Handshake protocol | **DONE** | SimpleHandshake with client ID |
| Exchange message processing | **DONE** | Both legacy and encrypted formats |
| Card update processing | **DONE** | Double Ratchet decryption |
| Send pending updates | **DONE** | Outbound queue processing |
| Acknowledgment handling | **DONE** | ACK sent for received messages |

---

## Remaining Feature Gaps

### HIGH Priority - Missing Core Features

| Feature | Platforms Missing | Effort | Plan Reference |
|---------|-------------------|--------|----------------|
| Edit display name | ALL | Low | Plan 1 |
| Edit field value | TUI, Desktop, iOS | Low | Plan 2 |
| Remove field | Desktop | Low | Plan 2 |
| Contact verification | Desktop, Android | Low | Plan 4 |
| Import backup UI | Desktop | Low | Plan 6 |
| Device management | All (partial) | High | Plan 8 |
| Recovery UI | Android, iOS | Medium | Plan 9 |

### MEDIUM Priority - UX Improvements

| Feature | Platforms Missing | Effort | Plan Reference |
|---------|-------------------|--------|----------------|
| Search contacts | TUI, Desktop | Low | Plan 3 |
| QR expiration timer | TUI, Desktop | Low | Plan 7 |
| Manual QR data input | TUI, iOS | Low | New |
| Relay URL config UI | TUI, Desktop | Low | New |
| Password strength | CLI, TUI, Android | Low | New |
| Bulk visibility | All except Desktop | Low | New |

### LOW Priority - Polish

| Feature | Platforms Missing | Effort |
|---------|-------------------|--------|
| Social network search UI | TUI, Desktop | Low |
| Offline indicator | TUI, Desktop | Low |
| Background sync | Desktop | Medium |

---

## Implementation Plans

### Plan 1: Edit Display Name (All Frontends)

**Files to modify**:
- CLI: Add `card name <new-name>` command
- TUI: Add edit dialog in home screen
- Desktop: Make name editable in Settings
- Android: Add edit button in Home
- iOS: Make name editable in Settings

**Implementation**:
1. Core already has `Identity::set_display_name()` and storage methods
2. Desktop already has `AppState::update_display_name()`
3. Just need UI wiring in each frontend

### Plan 2: Field Edit/Remove for Remaining Frontends

**Desktop** (remove field):
- `vauchi-desktop/ui/src/pages/Home.tsx` - Add delete button to field rows
- Backend `remove_field` command exists, wire to UI

**TUI** (edit field):
- `vauchi-tui/src/ui/home.rs` - Add edit dialog for fields

**iOS** (edit field):
- `Vauchi/Views/ContactCard.swift` - Add edit functionality

### Plan 3: Contact Search

**TUI**:
- Add search input in contacts screen
- Filter contact list based on query

**Desktop**:
- Add search bar in Contacts page
- Real-time filtering as user types

### Plan 4: Contact Verification

**Desktop**:
- Add verify button in contact detail view
- Show verification status badge

**Android**:
- Add verify option in contact detail
- Show fingerprint comparison dialog

### Plan 6: Import Backup UI

**Desktop**:
- Add import section in Settings/Backup page
- File picker for backup file
- Password input dialog
- Backend `import_backup` command exists

### Plan 7: QR Expiration Timer

**TUI**:
- Show countdown in exchange screen
- Auto-regenerate on expiration

**Desktop**:
- Add timer component to Exchange page
- Visual countdown indicator

### Plan 8: Device Management

**Scope**: Complete device linking across all frontends

**CLI** (complete remaining):
- `device join` - Join via link data
- `device complete` - Finish linking
- `device revoke` - Revoke device

**Mobile** (Android + iOS):
- Add device list screen
- Add link generation UI
- Add scan link UI
- Add revoke confirmation

**Desktop**:
- Add Devices page with list
- Add link generation
- Add revoke functionality

### Plan 9: Recovery UI for Mobile

**Android**:
- Complete RecoveryScreen with actual logic
- Add claim creation flow
- Add vouch flow for contacts

**iOS**:
- Create RecoveryView
- Integrate with VauchiMobile bindings
- Add claim/vouch workflows

---

## Deployment Checklist

### Phase 1: Deploy Relay (Immediate)

```bash
# From infra/hosts/bold-hopper/
cp .env.example .env
# Edit .env with KAMAL_REGISTRY_PASSWORD

# First-time setup
kamal setup

# Deploy relay
kamal deploy

# Verify
curl https://relay.vauchi.app/health
```

### Phase 2: Verify All Platforms Connect

| Platform | Test Command/Action |
|----------|---------------------|
| CLI | `vauchi sync` |
| TUI | Press 'n' to sync |
| Desktop | Click Sync in Settings |
| Android | Open app, trigger sync |
| iOS | Open app, trigger sync |

### Phase 3: Cross-Platform Exchange Test

Test exchange between every platform pair:
- [ ] Android ↔ iOS
- [ ] Android ↔ Desktop
- [ ] iOS ↔ Desktop
- [ ] CLI ↔ Mobile
- [ ] TUI ↔ Mobile

---

## Updated Feature Matrix

### After Completing This Plan

| Feature | CLI | TUI | Desktop | Android | iOS |
|---------|-----|-----|---------|---------|-----|
| Identity create/view | ✅ | ✅ | ✅ | ✅ | ✅ |
| Edit display name | ✅ | ✅ | ✅ | ✅ | ✅ |
| Card add/edit/remove | ✅ | ✅ | ✅ | ✅ | ✅ |
| QR exchange | ✅ | ✅ | ✅ | ✅ | ✅ |
| Contact search | ✅ | ✅ | ✅ | ✅ | ✅ |
| Contact verification | ✅ | ✅ | ✅ | ✅ | ✅ |
| Visibility control | ✅ | ✅ | ✅ | ✅ | ✅ |
| Relay sync | ✅ | ✅ | ✅ | ✅ | ✅ |
| Backup export/import | ✅ | ✅ | ✅ | ✅ | ✅ |
| Device management | ✅ | ✅ | ✅ | ✅ | ✅ |
| Recovery workflow | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## Priority Execution Order

### Immediate (Deploy relay today)
1. Deploy relay to relay.vauchi.app
2. Verify cross-platform sync

### This Week
1. Plan 2: Field edit/remove completion
2. Plan 3: Contact search
3. Plan 6: Import backup UI

### Next Week
1. Plan 1: Edit display name
2. Plan 4: Contact verification
3. Plan 7: QR timer

### Following Weeks
1. Plan 8: Device management
2. Plan 9: Recovery UI

---

## Success Criteria

- [ ] Relay deployed and accessible at wss://relay.vauchi.app
- [ ] All platforms can sync with relay
- [ ] Cross-platform exchange works for all pairs
- [ ] All HIGH priority features implemented
- [ ] Feature parity audit shows ✅ for all core features

---

## Related Documents

- `feature-parity-audit.md` - Detailed gap analysis
- `roadmap.md` - Overall project roadmap
- `P7-pre-production-checklist.md` - Launch checklist
- `infra/hosts/bold-hopper/README.md` - Deployment instructions

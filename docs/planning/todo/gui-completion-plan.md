# GUI Completion Plan - Full Implementation

## Goal
Make all Vauchi frontends feature-complete with full parity.

## TDD Approach
For each feature:
1. **Red**: Write failing test first
2. **Green**: Implement minimal code to pass
3. **Refactor**: Clean up while keeping tests green

---

## Phase 1: Desktop App Completion (Critical)

### 1.1 Desktop Sync Implementation
**Files**: `vauchi-desktop/src-tauri/src/commands/sync.rs`
**Current**: Returns placeholder data
**Required**:
- Connect to relay via WebSocket
- Send/receive encrypted updates
- Handle connection errors gracefully
- Update UI with real sync status

**Tests**:
- [ ] Test sync command returns real status
- [ ] Test connection error handling
- [ ] Test message send/receive

### 1.2 Desktop Last Sync Time
**Files**: `sync.rs`, app state
**Required**:
- Store last sync timestamp in local storage
- Load on app start
- Update after successful sync

**Tests**:
- [ ] Test timestamp persistence
- [ ] Test timestamp update on sync

### 1.3 Desktop Device Join Flow
**Files**: `vauchi-desktop/src-tauri/src/commands/devices.rs`
**Current**: TODO at line 135
**Required**:
- Generate device link QR
- Scan/enter link code
- Key agreement with linking device
- Receive shared identity backup
- Register device

**Tests**:
- [ ] Test link generation
- [ ] Test link parsing
- [ ] Test key agreement

### 1.4 Desktop Contact Verification UI
**Files**: `vauchi-desktop/ui/src/pages/Contacts.tsx`
**Required**:
- Add "Verify" button to contact detail
- Show fingerprint comparison dialog
- Call `verify_contact` backend command

**Tests**:
- [ ] Test verify button renders
- [ ] Test fingerprint display
- [ ] Test verification flow

### 1.5 Desktop Contact Search
**Files**: `vauchi-desktop/ui/src/pages/Contacts.tsx`
**Required**:
- Add search input field
- Filter contacts by name in real-time
- Clear search button

**Tests**:
- [ ] Test search input renders
- [ ] Test filtering works
- [ ] Test clear resets list

### 1.6 Desktop QR Expiration Timer
**Files**: `vauchi-desktop/ui/src/pages/Exchange.tsx`
**Required**:
- Display countdown timer
- Auto-refresh QR when expired
- Visual warning when expiring soon

**Tests**:
- [ ] Test timer displays
- [ ] Test countdown works
- [ ] Test auto-refresh

### 1.7 Desktop Relay URL Config
**Files**: `vauchi-desktop/ui/src/pages/Settings.tsx`
**Required**:
- Add relay URL input field
- Validate URL format (wss://)
- Save to config
- Test connection button

**Tests**:
- [ ] Test URL input renders
- [ ] Test validation
- [ ] Test save functionality

---

## Phase 2: TUI Completion

### 2.1 TUI Contact Search
**Files**: `vauchi-tui/src/ui/contacts.rs`, `handlers/input.rs`
**Required**:
- Press `/` to enter search mode
- Filter contacts as typing
- ESC to clear search

**Tests**:
- [ ] Test search mode activation
- [ ] Test filtering logic
- [ ] Test search clear

### 2.2 TUI QR Expiration Timer
**Files**: `vauchi-tui/src/ui/exchange.rs`
**Required**:
- Display countdown in exchange screen
- Show "Expired - Press R to refresh"
- Auto-refresh option

**Tests**:
- [ ] Test timer display
- [ ] Test expiration handling

### 2.3 TUI Relay URL Config
**Files**: `vauchi-tui/src/ui/settings.rs`, `backend.rs`
**Required**:
- Add relay URL option in settings
- Edit relay URL screen
- Validate and save

**Tests**:
- [ ] Test URL display
- [ ] Test URL edit
- [ ] Test validation

---

## Phase 3: iOS Completion

### 3.1 iOS Recovery UI
**Files**: `Vauchi/Views/RecoveryView.swift`
**Current**: Stub with "Not yet implemented"
**Required**:
- Create recovery claim UI
- Vouch for contact UI
- Recovery status display
- Proof verification UI

**Implementation Pattern**: Mirror Android's `RecoveryScreen.kt`

**Tests**:
- [ ] Test claim creation flow
- [ ] Test vouching flow
- [ ] Test status display

### 3.2 iOS Device Management Enable
**Files**: `Vauchi/Views/DevicesView.swift` (currently LinkedDevicesView)
**Current**: Disabled as "future feature"
**Required**:
- Enable device list
- Show current device
- Generate link code
- Link new device flow

**Tests**:
- [ ] Test device list display
- [ ] Test link generation

---

## Phase 4: Android Completion

### 4.1 Android Device Linking
**Files**: `MainActivity.kt`, `DevicesScreen.kt`
**Current**: `onGenerateLink = { null }` placeholder
**Required**:
- Implement link generation
- QR display for linking
- Scan to link flow

**Tests**:
- [ ] Test link generation
- [ ] Test QR display

---

## Implementation Order

### Week 1: Desktop Critical Path
1. [ ] Desktop sync implementation
2. [ ] Desktop last sync time
3. [ ] Desktop contact search
4. [ ] Desktop contact verification UI

### Week 2: Desktop + TUI
5. [ ] Desktop QR timer
6. [ ] Desktop relay config
7. [ ] TUI contact search
8. [ ] TUI QR timer
9. [ ] TUI relay config

### Week 3: Mobile
10. [ ] iOS recovery UI
11. [ ] iOS device management
12. [ ] Android device linking
13. [ ] Desktop device join flow

---

## Verification Checklist

After implementation:
- [ ] All platforms can edit display name
- [ ] All platforms can edit field values
- [ ] All platforms can remove fields
- [ ] All platforms can search contacts
- [ ] All platforms show QR expiration
- [ ] All platforms can configure relay
- [ ] Desktop sync actually works
- [ ] iOS recovery workflow complete
- [ ] Device linking works cross-platform
- [ ] All tests pass
- [ ] Clippy clean

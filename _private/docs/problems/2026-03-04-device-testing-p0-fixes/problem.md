# P0: Device Testing — Exchange UX Blockers

**Status**: planning
**Priority**: P0
**Owner**: Mattia Egloff + Claude Opus 4.6
**Date**: 2026-03-04
**Source**: Device test log (`_private/device-test-log.md`), T11 Android <-> iPhone QR Exchange

---

## Goals

Fix 5 P0-classified UX issues found during cross-platform device testing of `feature/core-completion-exchange`. These block a usable exchange flow on mobile.

---

## Issues Summary

| ID | Severity | Platform | Description | Root Cause |
|----|----------|----------|-------------|------------|
| UX-5 | Critical | iOS | App crashes on ultrasonic proximity | Missing `NSMicrophoneUsageDescription` in Info.plist |
| UX-6 | High | Both | Contact shows "New Contact" not actual name | QR only transmits crypto, card is placeholder `ContactCard::new("New Contact")` |
| UX-7 | Critical | Android | Scan QR button hidden off-screen | `ExchangeScreen.kt` Column has no scroll + `Spacer(weight(1f))` pushes button below fold |
| UX-1 | Medium | Android | Lock screen error is a dead end | No "Open Settings" button, misleading wording |
| UX-4 | Medium | iOS | Onboarding card preview shows empty card | Name from step 2 not passed to step 3 card preview |

UX-2 (Samsung S7 Keystore) and UX-3 (`pm clear` stale keys) are device-specific edge cases — tracked separately, not P0.

---

## Fix Plan

### Fix 1: UX-5 — iOS microphone permission (DONE)

**Status**: Fixed during testing session.
**Repo**: `ios/`
**File**: `ios/Vauchi/Info.plist`
**Change**: Added `NSMicrophoneUsageDescription` key.
**Remaining**: Commit the change.

### Fix 2: UX-7 — Android Scan QR button hidden

**Repo**: `android/`
**File**: `android/app/src/main/kotlin/com/vauchi/ui/ExchangeScreen.kt`

**Changes**:
1. **Line 84**: Add `.verticalScroll(rememberScrollState())` to Column modifier (match iOS `ScrollView` pattern)
2. **Line 87**: Change `fillMaxSize()` to `fillMaxWidth()` (let height be natural for scroll)
3. **Line 290**: Remove `Spacer(modifier = Modifier.weight(1f))` (prevents scroll from working)
4. Add import: `import androidx.compose.foundation.rememberScrollState` and `import androidx.compose.foundation.verticalScroll`

**Verification**: Build APK, install on M2, verify Scan QR button is visible and tappable below BLE card.

### Fix 3: UX-6 — Contact shows "New Contact" instead of actual name

**Repo**: `core/`, `cli/`, `android/`, `ios/` (via mobile bindings)

**Root cause**: The QR payload (`ExchangeQR`) only contains:
- Identity public key
- Ephemeral X3DH key
- Timestamp
- Signature

It does NOT contain the card or display name. After scanning, platforms create `ContactCard::new("New Contact")` as placeholder. The real card arrives later via relay sync — but on first exchange with no relay connectivity, the name is permanently "New Contact".

**Root cause detail**: The QR payload (`ExchangeQR`) only contains crypto material (identity key, ephemeral X3DH key, timestamp, audio challenge, signature — 189 bytes). No display name. The relay-mediated path (`EncryptedExchangeMessage`) already carries `display_name`, but QR-only exchange creates `ContactCard::new("New Contact")` as placeholder. The TUI relay path uses `payload.display_name` correctly (line 221 of `tui/src/backend/exchange.rs`), proving the architecture supports names — the QR path just skips it.

**Fix: Include display name in QR payload** (no alternative — this must work offline):
- Modify `ExchangeQR` struct to include `display_name: String` (plaintext, max 50 chars)
- The name is already publicly displayed on the physical card during in-person exchange, so including it in QR is not a privacy leak
- Update `ExchangeQR::generate()` and all serialization
- After scanning, use QR display_name to create `ContactCard::new(&qr.display_name)` instead of placeholder
- Bump QR version byte from 1 to 2 for backwards compatibility detection

**Files to modify**:
1. `core/vauchi-core/src/exchange/qr.rs` — add `display_name` to `ExchangeQR`, update `generate()` / `generate_with_timestamp()`, update serialization
2. `core/vauchi-core/src/exchange/session.rs` — use QR display_name in `ProcessQR` handler instead of placeholder
3. `cli/src/commands/exchange.rs:312` — use name from session state instead of `ContactCard::new("New Contact")`
4. `core/vauchi-mobile/src/exchange.rs` — verify mobile bindings pass name through
5. All platforms consuming `CompleteExchange` — verify card name propagates

**Tests**:
- Update `test_process_qr_rejects_large_clock_drift` and `test_process_qr_accepts_small_clock_drift` for new QR format
- New test: `test_qr_exchange_preserves_display_name`
- Verify existing tests still pass with updated QR struct

### Fix 4: UX-1 — Android lock screen error UX

**Repo**: `android/`
**File**: `android/app/src/main/kotlin/com/vauchi/ui/SecurityErrorScreen.kt` (or equivalent)

**Changes**:
1. Add "Open Settings" button that launches `Settings.ACTION_SECURITY_SETTINGS`
2. Improve wording: differentiate "no lock screen" vs "device locked" states
3. Add explanation: "Vauchi encrypts your contacts — device authentication is required to access them."
4. M2 case (device unlocked but Keystore auth needed): "Please authenticate with your PIN, fingerprint, or face to unlock your data."

### Fix 5: UX-4 — iOS onboarding card preview empty

**Repo**: `ios/`
**File**: iOS onboarding flow (likely `OnboardingView.swift` or `CardPreviewView.swift`)

**Change**: Pass the name entered in step 2 to the card preview in step 3. The preview should show the name and any default fields.

---

## MR Sequence

```
MR-A: ios/ — Fix UX-5 (microphone plist) + UX-4 (onboarding preview)
MR-B: android/ — Fix UX-7 (scroll layout) + UX-1 (lock screen UX)
MR-C: core/ — Fix UX-6 (display name in QR payload) — includes test updates
MR-D: cli/ — Fix UX-6 (use name from session instead of placeholder)
```

MR-C must merge first (core change), then MR-D + mobile apps rebuilt with new bindings.

---

## Verification

After all fixes:
1. Rebuild bindings: `./scripts/build-bindings.sh`
2. Build + install iOS and Android apps
3. Re-run T11: Android <-> iPhone QR exchange
4. Verify: contact name shows correctly on both devices
5. Verify: Scan QR button visible on Android
6. Verify: no crash on iOS ultrasonic
7. Run T12: Android <-> Android exchange
8. `just check core` — all tests pass

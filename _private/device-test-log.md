# Device Test Log — feature/core-completion-exchange

**Date**: 2026-03-04
**Branch**: `feature/core-completion-exchange` (9 commits ahead of main)
**Tester**: Mattia Egloff + Claude Opus 4.6
**Status**: Testing paused — 7 UX issues found, 5 classified P0
**Devices**:
- macOS desktop (D1)
- Linux desktop (D2)
- iPhone (M1)
- Android Phone 1 (M2)
- Android Phone 2 (M3)

---

## UX Issues Found

### UX-1: Android lock screen requirement — poor error UX
**Severity**: Medium
**Devices**: M2, M3
**Screenshots**: `android_device1.png`, `android_device2.png`
**Description**:
- M2 (has lock screen, device locked): Shows "Your device needs to be unlocked to access your data. Please unlock your device and tap Retry." — but the device *is* unlocked at this point (app is in foreground). Confusing wording.
- M3 (no lock screen configured): Shows "A secure lock screen (PIN, pattern, or biometric) is required to protect your data. Please set one up in your device Settings." — dead end with only a Retry button. User must leave the app, navigate to Settings, configure a lock screen, return, and tap Retry.

**Problems**:
1. Error screen is a dead end — no "Open Settings" button on M3, user must manually navigate
2. Wording on M2 is misleading — device is unlocked, the actual issue is Keystore authentication
3. No explanation of *why* a lock screen is needed (data encryption)
4. First-time users hitting this on launch will likely uninstall rather than configure device security
5. No visual branding on error screen — just a generic error, doesn't feel like the app

6. M3 is Android 8 (SDK 26) — "Swipe" lock doesn't count as secure. Users won't understand the difference between "lock screen" and "secure lock screen". The message should explicitly list what qualifies.

**Suggested fixes**:
- Add "Open Settings" deep link button on M3's screen (direct to `Settings.ACTION_SECURITY_SETTINGS`)
- Improve M2 wording: "Please authenticate with your PIN, fingerprint, or face to unlock your data."
- Add brief explanation: "Vauchi encrypts your contacts — device authentication is required to access them."
- Consider showing this as part of onboarding flow rather than an error screen

### UX-2: Samsung Galaxy S7 Keystore fails after configuring lock screen without reboot
**Severity**: High (blocks app usage entirely)
**Device**: M3 — Samsung Galaxy S7 (SM-G930F, Exynos, Android 8 / SDK 26)
**Description**:
After setting up PIN + fingerprint, the app still shows "A secure lock screen is required" error.
Root cause: `KeyGenParameterSpec.setUserAuthenticationRequired(true)` fails with `InvalidAlgorithmParameterException` because the Samsung TEE/Keymaster doesn't recognize the newly configured lock screen until the device is rebooted. The trust system reports `deviceLocked=0, strongAuthRequired=0x0` even with PIN+fingerprint configured. `settings get secure lockscreen.password_type` returns `null`.

**Impact**: Users who install Vauchi before setting up a lock screen will hit a dead end even after following the instructions — they must also reboot.

**Suggested fixes**:
1. Detect this edge case: if `KeyguardManager.isDeviceSecure()` returns false but user *has* configured lock screen → suggest reboot
2. Add "Restart device" as a suggested action in the error screen
3. Consider falling back to software-only encryption on devices where TEE is unreliable (with warning)
4. Test matrix: ensure Samsung Galaxy S-series on Android 8-9 are covered

### UX-3: `pm clear` does not clear Android Keystore entries — stale keys block app
**Severity**: Medium
**Device**: M3 — Samsung Galaxy S7
**Description**:
When the app creates a Keystore key in a bad state (no lock screen), `pm clear com.vauchi` wipes app data but NOT the Android Keystore entry (`USRSKEY_vauchi_storage_key`). On next launch, the app finds the stale key, tries to use it, and fails with `KeyStoreException: 112` / TEE error -30. Only a full `adb uninstall` clears Keystore entries.

**Impact**: Users (or support) can't fix the issue by clearing app data from Settings — they must fully uninstall and reinstall, losing any existing contacts.

**Suggested fixes**:
1. Catch `UnrecoverableKeyException` / error 112 in `getOrCreateMasterKey()` and **auto-delete the stale key** via `keyStore.deleteEntry()` before retrying
2. Add a "Reset App" button on the error screen that calls `deleteMasterKey()` + clears storage
3. On first launch, if key generation fails, don't persist a half-created key

### UX-4: iOS onboarding — card preview shows empty card (no name, no fields)
**Severity**: Medium
**Device**: M1 — iPhone (iOS 17.4.1)
**Description**:
During onboarding step 3/4 ("Your card"), the card preview shows only a blue avatar circle and "No additional info yet" — the name entered in the previous step is not displayed, and no fields are shown. User must tap "Edit card" to add fields before proceeding. The card preview should pre-populate with the name from step 2.

### UX-7: Android — Scan QR button not visible / unreachable
**Severity**: Critical (blocks exchange in scan direction)
**Devices**: M2, M3 (both Android)
**Description**:
The Exchange screen has no scrolling (`Column` without `verticalScroll`). The "Scan QR Code" button (line 335 of `ExchangeScreen.kt`) exists in code but is pushed off-screen by: QR card (280dp) + expiry text + ultrasonic row + `Spacer(weight(1f))` at line 290 + Bluetooth Exchange card (with 24dp padding). On real devices, the button is completely hidden below the viewport.

**Root cause**: `ExchangeScreen.kt:84` — parent `Column` uses `fillMaxSize()` but no `verticalScroll()` modifier. The `Spacer(weight(1f))` at line 290 fills all remaining space, pushing BLE card + scan button below the fold.

**Fix**:
1. Remove `Spacer(modifier = Modifier.weight(1f))` at line 290
2. Add `.verticalScroll(rememberScrollState())` to the Column modifier
3. Or: move Scan QR button above the BLE stub card (more prominent placement)

### UX-6: Both platforms — exchanged contact shows "New Contact" instead of actual name
**Severity**: High
**Devices**: M1 (iPhone), M2 (Android)
**Description**:
After QR exchange completes in both directions, contacts appear as "New Contact" on both platforms:
- iPhone shows: "New Contact" / "Not verified" with blue "N" avatar (should be "Bob")
- Android shows: "New Contact" / ID: ceeacf0dc74ddce1... (should be "Alice")

The card name is either not included in the QR payload or not parsed/stored correctly during the exchange flow.

**Impact**: Users won't know who they just exchanged with. Defeats the purpose of the card exchange.

**Suggested investigation**:
1. Check if the QR payload includes the card name or only crypto material
2. Check if `ExchangeQR::generate()` embeds the card, or only identity + ephemeral key
3. Check how `CompleteExchange(card)` receives the card — may need relay round-trip for card data
4. Android also doesn't show verification status (just raw ID) — UI gap

### UX-5: iOS crash — missing NSMicrophoneUsageDescription for ultrasonic proximity
**Severity**: Critical (blocks exchange entirely)
**Device**: M1 — iPhone (iOS 17.4.1)
**Crash log**: `Vauchi-2026-03-04-011912.ips`
**Description**:
After scanning Android's QR code, the app transitions to ultrasonic proximity verification and immediately crashes with `SIGABRT` / `TCC` privacy violation. The ultrasonic audio engine (`AVAudioEngine`) tries to access the microphone without `NSMicrophoneUsageDescription` in `Info.plist`.

**Call stack**:
```
ProximityVerificationView.attemptUltrasonicVerification()
→ VauchiViewModel.listenForProximityResponse(timeoutMs:)
→ MobileProximityVerifier.listenForResponse()
→ AudioProximityService.receiveSignal(timeoutMs:sampleRate:)
→ AVAudioEngine startAndReturnError: → TCC_CRASHING_DUE_TO_PRIVACY_VIOLATION
```

**Fix applied**: Added `NSMicrophoneUsageDescription` to `ios/Vauchi/Info.plist`:
`"Vauchi uses the microphone for ultrasonic proximity verification during contact exchange."`

---

## Pre-Test Setup

- [ ] Build branch on macOS: `cargo build -p vauchi-core --features testing`
- [ ] Build CLI on macOS: verify CLI can create identity and card
- [ ] Verify relay is reachable (or use local relay)

---

## Test Matrix

### T1: QR Exchange — Happy Path
**Devices**: D1 (macOS) <-> D2 (Linux)
**Steps**:
1. Create identity "Alice" on D1
2. Create identity "Bob" on D2
3. Alice generates QR, Bob scans
4. Bob generates QR, Alice scans
5. Both confirm proximity
6. Exchange completes

**Expected**: Both devices have each other as contacts.
**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T2: Battery Constraint Rejection
**Devices**: D1 (macOS) with mock low battery callback
**Steps**:
1. Create identity on D1
2. Configure MockPlatformCallbacks with battery_ok=false
3. Attempt exchange via apply_with_callbacks
4. Observe ExchangeError::LowBattery

**Expected**: Exchange blocked before key agreement.
**Status**: [ ]
**Result**: _pending_
**Notes**: This is testable via cargo test (unit test). For real device: check if platform reports battery < 20%.

---

### T3: Clock Drift Rejection
**Devices**: D1 (macOS) + D2 (Linux, clock offset 5 min)
**Steps**:
1. Offset D2 system clock by +5 minutes
2. D2 generates QR code (timestamp embedded in QR)
3. D1 scans D2's QR
4. D1's check_clock_drift() compares QR timestamp vs local time

**Expected**: ExchangeError::ClockDrift(300) error on D1.
**Status**: [ ]
**Result**: _pending_
**Notes**: MAX_CLOCK_DRIFT_SECONDS = 30 in code. 5 min offset should trigger.

---

### T4: Blocked Contact Rejection
**Devices**: D1 (macOS)
**Steps**:
1. Create identity "Alice" on D1
2. Exchange with "Bob" (complete successfully)
3. Block Bob via API
4. Attempt new exchange where Bob initiates
5. Use apply_with_callbacks_and_blocked with Bob's key in blocked list

**Expected**: ExchangeError::ContactBlocked.
**Status**: [ ]
**Result**: _pending_
**Notes**: Production path (apply_with_callbacks_and_blocked).

---

### T5: Search/Sort Contacts
**Devices**: D1 (macOS)
**Steps**:
1. Create 5+ contacts with varied names (Alice, Bob, Charlie, Diana, Eve)
2. Mark Alice and Charlie as fingerprint-verified
3. search_contacts_filtered("", default_filter, NameAsc)
4. search_contacts_filtered("", default_filter, NameDesc)
5. search_contacts_filtered("", verified_only, NameAsc)
6. search_contacts_filtered("ali", default_filter, NameAsc)

**Expected**:
- NameAsc: Alice, Bob, Charlie, Diana, Eve
- NameDesc: Eve, Diana, Charlie, Bob, Alice
- Verified only: Alice, Charlie
- Query "ali": Alice only

**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T6: Avatar Change Delta
**Devices**: D1 (macOS)
**Steps**:
1. Create two cards: old (with avatar bytes) and new (avatar removed)
2. Compute CardDelta between old and new
3. Verify delta.has_avatar_change() == true
4. Apply delta to old card
5. Verify card no longer has avatar

**Expected**: Avatar removal propagates through delta.
**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T7: Shareable Card Export
**Devices**: D1 (macOS)
**Steps**:
1. Create card with name "Test User" and fields (email, phone)
2. Call to_shareable_text()
3. Call to_shareable_qr_data()
4. Verify text format: "Test User\nemail: test@example.com\nphone: +1234567890"
5. Verify QR JSON has no internal IDs, only name + fields

**Expected**: Clean export with no internal identifiers.
**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T8: Merkle Tree Consistency
**Devices**: D1 (macOS) + D2 (Linux)
**Steps**:
1. Same contact set on both devices
2. Compute MerkleTree::from_contacts() on both
3. Compare root hashes

**Expected**: Identical root hashes for identical contact sets (order-independent).
**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T9: Cross-Platform QR Exchange
**Devices**: D1 (macOS) <-> M1 (iPhone)
**Steps**:
1. Create identity on macOS CLI
2. Create identity on iPhone app
3. Generate QR on macOS, scan from iPhone
4. Generate QR on iPhone, scan from macOS
5. Complete exchange

**Expected**: Contact appears on both platforms.
**Status**: [ ]
**Result**: _pending_
**Notes**: Requires iOS app built with this branch's core. May need to defer if bindings aren't regenerated yet.

---

### T10: Mesh Advertisement Privacy
**Devices**: D1 (macOS)
**Steps**:
1. Create MeshAdvertisement::new()
2. Verify name() == None
3. Verify public_key() == None
4. Serialize to bytes, verify only 16 bytes (session ID)
5. Create second advertisement, verify different session ID

**Expected**: No identity leakage in advertisement.
**Status**: [ ]
**Result**: _pending_
**Notes**: _pending_

---

### T11: Android <-> iPhone QR Exchange
**Devices**: M1 (iPhone) <-> M2 (Android Phone 1)
**Steps**:
1. Build bindings from feature branch: `./scripts/build-bindings.sh`
2. Build iOS app via Xcode, install on iPhone
3. Build Android app via Gradle, install on Android Phone 1
4. Create identity on iPhone
5. Create identity on Android
6. iPhone generates QR, Android scans
7. Android generates QR, iPhone scans
8. Both confirm proximity
9. Exchange completes

**Expected**: Contact appears on both platforms with correct name and fields.
**Status**: [x]
**Result**: PARTIAL PASS — exchange works cross-platform, contacts appear on both devices, but with issues
**Notes**:

**Prerequisites** (all completed):
- [x] `sdkmanager --install "ndk;26.1.10909125"`
- [x] `rustup target add aarch64-linux-android x86_64-linux-android`
- [x] `./scripts/build-bindings.sh` (both platforms)
- [x] Xcode build + install on iPhone
- [x] Gradle build + install on Android

**Execution log**:
1. iPhone (Alice) scanned Android (Bob) QR → iOS crashed (UX-5: missing NSMicrophoneUsageDescription). Fixed Info.plist, rebuilt, reinstalled.
2. iPhone scanned Android QR again → ultrasonic challenge appeared, mic permission granted, contact added as "New Contact" (not "Bob") — UX-6. Unverified.
3. Android needed to scan iPhone QR but Scan button hidden off-screen (UX-7). Triggered via adb blind tap.
4. Android scanned iPhone QR → ultrasonic verification failed first attempt → "Exchange failed". Retry succeeded.
5. Both devices now show "New Contact" in their contacts list.

**Issues found during T11**:
- UX-5: iOS crash — missing NSMicrophoneUsageDescription (FIXED)
- UX-6: Contact name shows "New Contact" on both platforms — card name not in QR payload
- UX-7: Android Scan QR button hidden off-screen (non-scrollable Column + weight spacer)

---

### T12: Android <-> Android QR Exchange
**Devices**: M2 (Android Phone 1) <-> M3 (Android Phone 2)
**Steps**:
1. Install app on both Android phones (same APK from T11 build)
2. Create identity "Charlie" on M2
3. Create identity "Diana" on M3
4. M2 generates QR, M3 scans
5. M3 generates QR, M2 scans
6. Both confirm proximity
7. Exchange completes

**Expected**: Contact appears on both devices.
**Status**: [ ]
**Result**: _pending_
**Notes**: Both devices get the same APK. Tests Android-only path.

---

## Automated Test Results (cargo test, 2026-03-04)

All 54 integration tests pass across 5 test files.

| Test | Status | Pass/Fail | Notes |
|------|--------|-----------|-------|
| T1   | Manual | Pending   | Needs 2 devices with relay |
| T2   | Auto   | PASS      | test_exchange_blocked_when_battery_insufficient (10 exchange constraint tests) |
| T3   | Auto   | PASS      | test_process_qr_rejects_large_clock_drift + manual clock offset test pending |
| T4   | Auto   | PASS      | test_production_blocked_contact_rejection + test_exchange_rejects_blocked_contact |
| T5   | Auto   | PASS      | 10 search/sort tests (NameAsc, NameDesc, RecentFirst, VerificationStatus, filters) |
| T6   | Auto   | PASS      | test_avatar_removal_creates_delta + test_avatar_delta_applied_correctly (9 tests) |
| T7   | Auto   | PASS      | test_shareable_text_format + test_shareable_qr_data_format |
| T8   | Auto   | PASS      | test_merkle_tree_from_contacts_deterministic + test_merkle_tree_order_independent (11 tests) |
| T9   | Manual | Pending   | Needs iOS app rebuilt with this branch's core |
| T10  | Auto   | PASS      | test_mesh_advertisement_has_no_identity_info + 14 mesh tests |
| T11  | Manual | PARTIAL   | Exchange works, contacts appear — but name missing (UX-6), scan button hidden (UX-7), iOS crash fixed (UX-5) |
| T12  | Manual | Blocked   | Blocked by UX-7 (Android scan button hidden). M3 needs identity creation. |

---

## Session Summary (2026-03-04)

**Testing paused** after T11 (Android <-> iPhone QR Exchange). 7 UX issues found, 5 classified P0.

### P0 Issues (must fix before release)

| ID | Fix Status | Repo | Description |
|----|-----------|------|-------------|
| UX-5 | **FIXED** | ios/ | Missing `NSMicrophoneUsageDescription` — crash on ultrasonic proximity |
| UX-6 | Plan ready | core/ + all | Contact shows "New Contact" — QR lacks display name |
| UX-7 | Plan ready | android/ | Scan QR button hidden — Column not scrollable |
| UX-1 | Plan ready | android/ | Lock screen error dead end — no Settings button |
| UX-4 | Plan ready | ios/ | Onboarding card preview empty — name not passed |

### Non-P0 Issues (tracked separately)

| ID | Severity | Description |
|----|----------|-------------|
| UX-2 | High | Samsung S7 Keystore fails without reboot after lock screen setup |
| UX-3 | Medium | `pm clear` doesn't clear Android Keystore entries |

### Fix plan

See: `_private/docs/problems/2026-03-04-device-testing-p0-fixes/problem.md`

### Remaining tests after P0 fixes

- T11: Re-run with fixes (verify name shows, scan works, no crash)
- T12: Android <-> Android (M2 <-> M3)
- T9: macOS CLI <-> iPhone
- T1: Desktop QR exchange (D1 <-> D2)
- T3: Manual clock drift test

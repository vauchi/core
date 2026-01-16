# Camera Scanning

**Status**: Planned

## Overview

Native camera integration for QR code scanning to replace manual paste workflow.

## Current State

- QR codes are generated and displayed correctly
- Users must manually paste scanned QR data
- Exchange completes successfully with pasted data

## Requirements

### Functional
- Request camera permission on first use
- Display camera preview in Exchange screen
- Detect and decode QR codes in real-time
- Process scanned QR data automatically
- Handle permission denied gracefully

### Non-Functional
- Camera preview < 1 second startup
- QR detection < 500ms
- Minimal battery impact

## Implementation Options

### Option A: ML Kit (Recommended)
- Google's ML Kit Barcode Scanning
- Well-documented, reliable
- Handles all QR formats

### Option B: ZXing
- Open source, widely used
- Already in project for QR generation
- May need camera integration library

### Option C: CameraX + ZXing
- CameraX for camera handling
- ZXing for QR decoding
- Most control, more code

## Files to Modify

| File | Changes |
|------|---------|
| `build.gradle.kts` | Add ML Kit or camera dependencies |
| `AndroidManifest.xml` | Camera permission (already present) |
| `ui/ExchangeScreen.kt` | Add camera preview composable |
| `MainActivity.kt` | Handle permission flow |

## Acceptance Criteria

- [ ] User can tap "Scan" to open camera
- [ ] Camera preview shows in Exchange screen
- [ ] QR codes detected within 500ms
- [ ] Scanned data auto-processed
- [ ] Permission denied shows fallback (manual paste)
- [ ] Works in low-light conditions

# Camera Scanning

**Priority**: High | **Complexity**: Low

## Current State
QR codes display correctly. Users manually paste scanned data.

## Goal
Native camera integration for automatic QR scanning.

## Options
1. **ML Kit** (Recommended) - Google's barcode scanning
2. **CameraX + ZXing** - More control, more code

## Requirements
- Camera permission handling
- Real-time QR detection (< 500ms)
- Fallback to manual paste if denied

## Files to Modify
- `build.gradle.kts` - Add ML Kit dependency
- `ui/ExchangeScreen.kt` - Camera preview
- `MainActivity.kt` - Permission flow

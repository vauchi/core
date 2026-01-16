# Camera Scanning ✅

## Implementation
- **CameraX** for camera preview
- **ML Kit** for barcode scanning
- Permission handling with runtime request

## Files Added/Modified
- `ui/QrScannerScreen.kt` - Camera preview and QR analyzer
- `build.gradle.kts` - CameraX and ML Kit dependencies
- `MainActivity.kt` - Navigation wiring

## Features
- Real-time QR detection
- Permission denied fallback UI
- Validates `wb://` prefix before accepting
- Auto-navigates after successful scan

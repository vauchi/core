# iOS App

**Phase**: 1 | **Complexity**: Medium

## Goal
Native iOS app with feature parity to Android.

## Approach
- SwiftUI for UI
- UniFFI bindings from webbook-mobile crate
- Reuse all core logic from Rust

## Requirements
- All functional features from Android
- Camera scanning (AVFoundation or Vision)
- Keychain for secure key storage
- Background sync via BGTaskScheduler

## Files to Create
- `webbook-ios/` - Xcode project
- Swift wrappers for UniFFI bindings
- SwiftUI screens matching Android

## Dependencies
- Xcode + iOS SDK
- UniFFI Swift bindings generation

# Multi-Device Sync / Device Linking

**Phase**: 1 | **Complexity**: High

## Goal
Users can link multiple devices to the same identity and keep them in sync.

## Approach
- Device registration with identity
- Sync protocol extension for device-to-device updates
- Conflict resolution strategy

## Key Decisions
- **Device keys**: Each device has own keypair, linked to identity
- **Sync scope**: Contacts, card fields, settings
- **Conflict resolution**: Last-write-wins with vector clocks

## Requirements
- Link new device via QR (secure transfer of identity seed or derived key)
- Sync contacts and card across devices
- Handle offline/online transitions gracefully
- Revoke device access

## Architecture Impact
- Extend `webbook-core` identity module for multi-device
- New sync message types for device registration
- Storage schema changes for device metadata

## Security Considerations
- Device linking must be authenticated (QR in-person or PIN)
- Each device should have forward secrecy with contacts
- Device revocation must propagate to contacts

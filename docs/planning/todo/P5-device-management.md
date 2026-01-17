# Plan: Device Management Completion

**Priority**: HIGH
**Effort**: 1-2 weeks
**Impact**: Multi-device support (core feature)

## Problem Statement

Device linking is partially implemented in CLI but incomplete across all frontends:
- CLI: Link/join/revoke are stubs with warnings
- TUI: Device list is placeholder
- Desktop: Can list and generate link, but cannot join or revoke
- Android: Can generate link, cannot join
- iOS: Device section is disabled ("future feature")

## Success Criteria

- [ ] Complete device linking flow in CLI (as reference implementation)
- [ ] Mobile apps can join an existing identity via QR
- [ ] All frontends can list linked devices
- [ ] Revocation works and propagates to contacts
- [ ] Tests cover full device lifecycle

## Current State Analysis

### What Exists

**Core** (`webbook-core/src/exchange/device_link.rs`):
- `DeviceLinkQR::generate()` - Creates QR with link key
- `DeviceLinkQR::from_data_string()` - Parses QR
- `SeedTransfer` - Encrypted master seed transfer
- `DeviceInfo`, `DeviceRegistry` structs
- Expiration (10 minutes)
- Signature verification

**Gap**: The actual device joining and registry sync is incomplete.

### What's Missing

1. **CLI `device complete`** - Shows warning, doesn't do actual linking
2. **CLI `device finish`** - Has placeholder identity creation
3. **CLI `device revoke`** - Shows warning, no implementation
4. **Mobile apps** - Cannot complete device join flow
5. **Registry broadcast** - Devices don't sync their registries

## Implementation

### Phase 1: CLI Reference Implementation

**Task 1.1: Complete `device complete` command**

**File**: `webbook-cli/src/commands/device.rs`

```rust
async fn execute_device_complete(request: &str) -> Result<()> {
    let webbook = load_webbook()?;

    // Parse the link request from new device
    let request = DeviceLinkRequest::from_data_string(request)
        .context("Invalid link request")?;

    // Verify it matches our pending link QR
    let pending_link = webbook.get_pending_device_link()
        .context("No pending device link")?;

    if request.identity_public_key != pending_link.identity_public_key() {
        bail!("Request doesn't match pending link");
    }

    // Create seed transfer encrypted with link key
    let seed_transfer = SeedTransfer::create(
        webbook.identity(),
        pending_link.link_key(),
    )?;

    // Register new device in our registry
    let new_device = DeviceInfo::new(
        request.device_id,
        request.device_name,
        request.device_type,
    );
    webbook.device_registry().add_device(new_device)?;
    webbook.save()?;

    // Output the response for the new device
    let response = seed_transfer.to_data_string();
    println!("Send this to the new device:\n{}", response);

    Ok(())
}
```

**Task 1.2: Complete `device finish` command**

```rust
async fn execute_device_finish(response: &str) -> Result<()> {
    // Load pending link key (saved during `device join`)
    let pending = load_pending_link()?;

    // Parse seed transfer
    let transfer = SeedTransfer::from_data_string(response)
        .context("Invalid seed transfer")?;

    // Decrypt master seed using link key
    let master_seed = transfer.decrypt(&pending.link_key)?;

    // Recreate identity from seed
    let identity = Identity::from_seed(&master_seed, &pending.device_info)?;

    // Initialize webbook with this identity
    let webbook = WebBook::new_with_identity(identity)?;
    webbook.save()?;

    println!("Device successfully linked!");
    println!("Run 'webbook sync' to fetch your contacts.");

    Ok(())
}
```

**Task 1.3: Implement `device revoke`**

```rust
async fn execute_device_revoke(device_id: &str) -> Result<()> {
    let mut webbook = load_webbook()?;

    // Create revocation certificate
    let cert = webbook.identity().create_revocation_certificate(device_id)?;

    // Update local registry
    webbook.device_registry().revoke_device(device_id, &cert)?;

    // Queue registry update for broadcast to contacts
    webbook.sync_manager().queue_registry_update()?;

    webbook.save()?;

    println!("Device {} revoked.", device_id);
    println!("Run 'webbook sync' to notify contacts.");

    Ok(())
}
```

### Phase 2: Mobile Implementation

**Task 2.1: Add UniFFI bindings**

**File**: `webbook-mobile/src/lib.rs`

```rust
impl WebBookMobile {
    /// Parse a device link QR and prepare to join
    pub fn parse_device_link(&self, qr_data: String) -> Result<DeviceLinkInfo, MobileError> {
        let link = DeviceLinkQR::from_data_string(&qr_data)?;
        Ok(DeviceLinkInfo {
            identity_pk: hex::encode(link.identity_public_key()),
            expires_at: link.timestamp() + 600,
            is_expired: link.is_expired(),
        })
    }

    /// Generate a link request to send to existing device
    pub fn generate_link_request(&self, device_name: String) -> Result<String, MobileError> {
        // Generate request with this device's info
        let request = DeviceLinkRequest::new(device_name)?;
        Ok(request.to_data_string())
    }

    /// Complete joining by processing seed transfer
    pub fn complete_device_join(&self, seed_transfer: String) -> Result<(), MobileError> {
        // ... similar to CLI device finish
    }
}
```

**Task 2.2: Android UI**

**File**: `webbook-android/.../ui/DevicesScreen.kt`

- Add "Join Existing Identity" flow
- QR scanner to scan link QR from another device
- Display request data to show on existing device
- Input field for seed transfer response

**Task 2.3: iOS UI**

**File**: `webbook-ios/WebBook/Views/SettingsView.swift`

- Enable device linking section (currently disabled)
- Add QR scanner for joining
- Add UI for receiving seed transfer

### Phase 3: Registry Sync

**Task 3.1: Include device registry in sync protocol**

**File**: `webbook-core/src/sync/mod.rs`

Add registry updates to sync payload:
- Send registry when changed
- Receive registry updates from other devices
- Merge registries (trust signatures, handle revocations)

### Phase 4: Testing

**File**: `webbook-core/tests/device_tests.rs`

```rust
#[test]
fn test_full_device_linking_flow() {
    // Device A (existing)
    let mut device_a = create_identity("Alice");
    let link_qr = device_a.generate_device_link();

    // Device B (new)
    let parsed = DeviceLinkQR::from_data_string(&link_qr.to_data_string()).unwrap();
    assert!(!parsed.is_expired());

    let request = DeviceLinkRequest::new("Phone".into()).unwrap();

    // Device A processes request
    let transfer = device_a.complete_device_link(&request).unwrap();

    // Device B completes join
    let device_b = complete_device_join(&transfer).unwrap();

    // Both should have same identity
    assert_eq!(device_a.identity_pk(), device_b.identity_pk());
}

#[test]
fn test_device_revocation() {
    // Setup two linked devices
    let (device_a, device_b) = link_two_devices();

    // Device A revokes Device B
    device_a.revoke_device(device_b.device_id()).unwrap();

    // Verify registry updated
    let registry = device_a.device_registry();
    assert!(registry.is_revoked(device_b.device_id()));
}
```

## Implementation Order

1. **Week 1**: CLI completion (Tasks 1.1-1.3)
2. **Week 1**: Core tests (Phase 4)
3. **Week 2**: Mobile bindings (Task 2.1)
4. **Week 2**: Android UI (Task 2.2)
5. **Week 2**: iOS UI (Task 2.3)
6. **Future**: Registry sync (Phase 3)

## Dependencies

- Core device link module (exists)
- Double Ratchet for registry encryption
- Sync protocol for registry broadcast

## Risks

- **Complexity**: Device sync requires careful conflict resolution
- **Security**: Master seed transfer must be secure (one-time, encrypted)
- **UX**: Multi-step flow may confuse users

## Mitigation

- Start with CLI to validate flow before mobile
- Extensive logging during development
- Clear user guidance at each step

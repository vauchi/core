// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-keychain setup for crypto-shredding on mobile.
//!
//! Slice 32i.2 (2026-06-01) retired the 4 keychain-bound shred methods
//! (`soft_shred`, `cancel_shred`, `hard_shred`, `panic_shred`) from this
//! `VauchiPlatform` surface once their replacement landed on
//! `PlatformAppEngine`'s `DomainCommand` dispatch (B7 Phase 1a/1b:
//! `SoftShred` / `CancelShred` / `HardShred` / `PanicShred`). They had
//! zero hand-written frontend consumers (the fence in the prior header).
//!
//! What remains here is the `set_platform_keychain` setter + the
//! `MobilePlatformKeychain` callback interface (defined in `lib.rs`). The
//! `MobileShredToken` / `MobileShredReport` types live in
//! `types/security.rs` and back both the new PAE dispatch results and the
//! `widget_panic_shred` free fn, so they (and this setter, used by the
//! panic widget) stay.

use std::sync::Arc;

use super::{MobilePlatformKeychain, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    /// Set the platform keychain for crypto-shredding operations.
    ///
    /// The keychain provides access to the platform's native secure
    /// storage (iOS Keychain, Android KeyStore) for SMK management. The
    /// shred operations themselves now run through `PlatformAppEngine`'s
    /// `DomainCommand` dispatch (B7); this setter persists for the
    /// `VauchiPlatform` keychain slot read by `widget_panic_shred`.
    pub fn set_platform_keychain(&self, keychain: Box<dyn MobilePlatformKeychain>) {
        // NOTE: Silent failure on lock poison means subsequent shred operations
        // will fail with "keychain not set" instead of surfacing the root cause.
        // Acceptable for now since poison here requires a prior panic in the same
        // process, which is already a terminal state on mobile.
        let Ok(mut lock) = self.platform_keychain.lock() else {
            return;
        };
        *lock = Some(Arc::from(keychain));
    }
}

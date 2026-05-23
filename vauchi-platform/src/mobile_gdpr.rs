// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keychain-bound crypto-shredding for mobile.
//!
//! After slice 32i Phase 1 (2026-05-17), the GDPR / deletion / consent
//! surface migrated to `lib.rs::impl VauchiPlatform` (G4b path) — those
//! 11 methods have `DomainCommand` variants already wired in
//! `platform_app_engine.rs` and zero in-tree frontend consumers, but
//! `lib.rs`'s internal test block still calls them directly
//! (~19 call sites across 5 tests). The relocation collapsed the
//! `mobile_pub_fns` audit counter from 156 to 145 without forcing a
//! same-MR migration of those tests; per slice 32g-B precedent the
//! `wb.*` call sites continue to resolve.
//!
//! The 4 shred methods (`soft_shred`, `cancel_shred`, `hard_shred`,
//! `panic_shred`) + the `set_platform_keychain` setter stay here
//! pending the B7 keychain-plumbing batch — `PlatformAppEngine` does
//! not yet hold a `MobilePlatformKeychain` reference, so they cannot
//! be routed through `DomainCommand` (note at
//! `domain_command.rs:80-83`). Slice 32i.2 retires them once that
//! plumbing lands. `verify_shred` retired 2026-05-23 (Track A) —
//! zero hand-written consumers and `MobileShredVerification` had no
//! other producer, so the type's `From` impl was retired too.
//! Record: `done/2026-05-17-slice-32i-mobile-gdpr-partial-retirement/`.

use std::sync::Arc;

use super::error::{MobileError, lock_or};
use super::types::{MobileShredReport, MobileShredToken};
use super::{MobilePlatformKeychain, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === Crypto-Shredding Operations ===

    /// Set the platform keychain for crypto-shredding operations.
    ///
    /// Must be called before any shred operation. The keychain provides
    /// access to the platform's native secure storage (iOS Keychain,
    /// Android KeyStore) for SMK management.
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

    /// Schedule crypto-shredding with 7-day grace period (Soft Shred).
    ///
    /// Returns a token that must be passed to `hard_shred()` after the grace period.
    /// Also refreshes the pre-signed messages file for future panic shred.
    ///
    /// Requires `set_platform_keychain()` to be called first.
    pub fn soft_shred(&self) -> Result<MobileShredToken, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let token = manager.soft_shred().map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })?;
        Ok(MobileShredToken::from(&token))
    }

    /// Cancel a scheduled shred during the grace period.
    pub fn cancel_shred(&self, token: MobileShredToken) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        manager
            .cancel_shred(core_token)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        Ok(())
    }

    /// Execute irreversible crypto-shredding (Hard Shred).
    ///
    /// Requires the grace period to have elapsed. Destroys all key material,
    /// secure-deletes the database, and removes all local data.
    ///
    /// **WARNING**: This operation is irreversible. All identity data will be
    /// permanently destroyed.
    pub fn hard_shred(&self, token: MobileShredToken) -> Result<MobileShredReport, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let _ = lock_or(&self.pinned_cert_pem)?.clone();

        let (purge_res, rev_res) = self.build_shred_senders(&identity.public_id());
        let (mut purge_sender, purge_error) = match purge_res {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e)),
        };
        let (mut revocation_sender, rev_error) = match rev_res {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e)),
        };

        let report = manager
            .hard_shred(
                core_token,
                purge_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::PurgeSender),
                revocation_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::RevocationSender),
            )
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let mut mobile_report = MobileShredReport::from(&report);
        if let Some(err) = purge_error {
            mobile_report.purge_failed = true;
            mobile_report.purge_error = Some(err);
        }
        if let Some(err) = rev_error {
            mobile_report.revocation_failed = true;
            mobile_report.revocation_error = Some(err);
        }
        Ok(mobile_report)
    }

    /// Execute immediate crypto-shredding without grace period (Panic Shred).
    ///
    /// Loads pre-signed messages before destroying keys, then sends them
    /// best-effort. Use only in emergencies.
    ///
    /// **WARNING**: This operation is irreversible and immediate. No grace period.
    pub fn panic_shred(&self) -> Result<MobileShredReport, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let _ = lock_or(&self.pinned_cert_pem)?.clone();

        let (purge_res, rev_res) = self.build_shred_senders(&identity.public_id());
        let (mut purge_sender, purge_error) = match purge_res {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e)),
        };
        let (mut revocation_sender, rev_error) = match rev_res {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e)),
        };

        let report = manager
            .panic_shred(
                purge_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::PurgeSender),
                revocation_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::RevocationSender),
            )
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let mut mobile_report = MobileShredReport::from(&report);
        if let Some(err) = purge_error {
            mobile_report.purge_failed = true;
            mobile_report.purge_error = Some(err);
        }
        if let Some(err) = rev_error {
            mobile_report.revocation_failed = true;
            mobile_report.revocation_error = Some(err);
        }
        Ok(mobile_report)
    }
}

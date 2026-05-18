// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity operations for mobile.
//!
//! After slice 32j Phase 1 (2026-05-18), the 14 aha-moment + demo-contact
//! methods were retired (all had `DomainCommand` variants wired in PAE
//! and zero binding-side callers — core/vauchi-core tests hit the
//! `vauchi_core::api::Vauchi` namesakes, not these binding wrappers).
//! The 5 remaining methods (`has_identity`, `create_identity`,
//! `get_public_id`, `get_own_fingerprint`, `get_display_name`) are
//! test infrastructure with many `wb.*` callers in `lib.rs` internal
//! tests + `tests/it/` + `benches/ffi_benchmarks.rs`; they retire in
//! slice 32j Phase 2 via G4b relocation to `lib.rs` (consistent with
//! slice 32g-B + 32i precedent).

use vauchi_core::{ContactCard, Identity};

use super::error::{MobileError, lock_or};
use super::{IdentityData, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === Identity Operations ===

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        {
            let Ok(data) = self.identity_data.lock() else {
                return false;
            };
            if data.is_some() {
                return true;
            }
        }

        if let Ok(storage) = self.open_storage()
            && let Ok(Some((backup_data, display_name))) = storage.load_identity()
        {
            let identity_data = IdentityData {
                backup_data,
                display_name,
            };
            let Ok(mut lock) = self.identity_data.lock() else {
                return false;
            };
            *lock = Some(identity_data);
            return true;
        }

        false
    }

    /// Create a new identity.
    pub fn create_identity(&self, display_name: String) -> Result<(), MobileError> {
        {
            let data = lock_or(&self.identity_data)?;
            if data.is_some() {
                return Err(MobileError::Other {
                    detail: "Already initialized".to_string(),
                });
            }
        }

        let identity = Identity::create(
            &display_name,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );

        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;

        let backup_data = backup.as_bytes().to_vec();

        let storage = self.open_storage()?;
        storage.save_identity(&backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data,
            display_name: display_name.clone(),
        };
        *lock_or(&self.identity_data)? = Some(identity_data);

        let card = ContactCard::new(&display_name);
        storage.save_own_card(&card)?;

        Ok(())
    }

    /// Get public ID.
    pub fn get_public_id(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.public_id())
    }

    /// Get formatted fingerprint of own identity public key.
    ///
    /// Returns the fingerprint as 16 groups of 4 uppercase hex characters,
    /// suitable for display and manual comparison with contacts.
    pub fn get_own_fingerprint(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        let hex = hex::encode(identity.signing_public_key());
        Ok(hex
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase())
    }

    /// Get display name.
    pub fn get_display_name(&self) -> Result<String, MobileError> {
        let storage = self.open_storage()?;
        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        Ok(card.display_name().to_string())
    }
}

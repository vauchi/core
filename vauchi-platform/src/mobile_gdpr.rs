// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR operations, crypto-shredding, and consent management for mobile.

use std::sync::Arc;

use super::error::MobileError;
use super::types::{
    MobileConsentRecord, MobileConsentStatus, MobileConsentType, MobileDeletionInfo,
    MobileGdprExport, MobileShredReport, MobileShredStatus, MobileShredToken,
    MobileShredVerification,
};
use super::{MobilePlatformKeychain, MobileRevocationSender, VauchiPlatform};

#[uniffi::export]
impl VauchiPlatform {
    // === GDPR Operations ===

    /// Export all user data for GDPR compliance.
    pub fn export_gdpr_data(&self) -> Result<MobileGdprExport, MobileError> {
        let storage = self.open_storage()?;
        let export = vauchi_core::api::export_all_data(&storage)?;

        let json_data = serde_json::to_string_pretty(&export)
            .map_err(|e| MobileError::GdprError(e.to_string()))?;

        Ok(MobileGdprExport {
            json_data,
            exported_at: export.exported_at,
            version: export.version,
        })
    }

    /// Schedule identity deletion with 7-day grace period.
    pub fn schedule_identity_deletion(&self) -> Result<MobileDeletionInfo, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        manager
            .schedule_deletion()
            .map_err(|e| MobileError::DeletionNotAllowed(e.to_string()))?;

        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(MobileDeletionInfo::from(&state))
    }

    /// Cancel a scheduled identity deletion.
    pub fn cancel_identity_deletion(&self) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        manager
            .cancel_deletion()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(())
    }

    /// Execute identity deletion (only after grace period).
    ///
    /// Generates revocation messages for all contacts and shreds CEKs.
    /// Returns the number of revocation messages generated (caller should
    /// arrange relay delivery).
    pub fn execute_identity_deletion(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let result = manager
            .execute_deletion(&identity)
            .map_err(|e| MobileError::DeletionNotAllowed(e.to_string()))?;
        Ok(result.revocations.len() as u32)
    }

    /// Get current deletion state.
    pub fn get_deletion_state(&self) -> Result<MobileDeletionInfo, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(MobileDeletionInfo::from(&state))
    }

    // === Crypto-Shredding Operations ===

    /// Set the platform keychain for crypto-shredding operations.
    ///
    /// Must be called before any shred operation. The keychain provides
    /// access to the platform's native secure storage (iOS Keychain,
    /// Android KeyStore) for SMK management.
    pub fn set_platform_keychain(&self, keychain: Box<dyn MobilePlatformKeychain>) {
        let mut lock = self.platform_keychain.lock().unwrap();
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
        let token = manager
            .soft_shred()
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
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
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
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

        let (mut revocation_sender, rev_error) = match MobileRevocationSender::new(
            &self.relay_url,
            &identity.public_id(),
            self.pinned_cert_pem.lock().unwrap().clone(),
        ) {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let report = manager
            // PurgeSender deferred: requires relay-side purge endpoint support.
            .hard_shred(
                core_token,
                None,
                revocation_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::RevocationSender),
            )
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        let mut mobile_report = MobileShredReport::from(&report);
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

        let (mut revocation_sender, rev_error) = match MobileRevocationSender::new(
            &self.relay_url,
            &identity.public_id(),
            self.pinned_cert_pem.lock().unwrap().clone(),
        ) {
            Ok(sender) => (Some(sender), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let report = manager
            // PurgeSender deferred: requires relay-side purge endpoint support.
            .panic_shred(
                None,
                revocation_sender
                    .as_mut()
                    .map(|s| s as &mut dyn vauchi_core::api::RevocationSender),
            )
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        let mut mobile_report = MobileShredReport::from(&report);
        if let Some(err) = rev_error {
            mobile_report.revocation_failed = true;
            mobile_report.revocation_error = Some(err);
        }
        Ok(mobile_report)
    }

    /// Verify that shredding was successful by checking for residual data.
    ///
    /// Returns verification results showing which items were confirmed destroyed.
    pub fn verify_shred(&self) -> Result<MobileShredVerification, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let verification = manager.verify_shred();
        Ok(MobileShredVerification::from(&verification))
    }

    /// Get current shred status.
    ///
    /// Returns whether no shred is in progress, one is scheduled (with remaining
    /// time), or has been executed.
    pub fn shred_status(&self) -> Result<MobileShredStatus, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::ShredError(e.to_string()))?;

        match state {
            vauchi_core::storage::DeletionState::None => Ok(MobileShredStatus::None),
            vauchi_core::storage::DeletionState::Scheduled { execute_at, .. } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let remaining = execute_at.saturating_sub(now);
                Ok(MobileShredStatus::Scheduled {
                    remaining_secs: remaining,
                })
            }
            vauchi_core::storage::DeletionState::Executed { .. } => Ok(MobileShredStatus::Executed),
        }
    }

    // === Consent Operations ===

    /// Grant consent for a specific type.
    pub fn grant_consent(&self, consent_type: MobileConsentType) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        manager.grant(vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(())
    }

    /// Revoke consent for a specific type.
    pub fn revoke_consent(&self, consent_type: MobileConsentType) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        manager.revoke(vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(())
    }

    /// Check whether consent is currently granted for a type.
    pub fn check_consent(&self, consent_type: MobileConsentType) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        let granted = manager.check(&vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(granted)
    }

    /// Get the aggregated consent status for a specific type.
    ///
    /// Returns granted state, last change timestamp, and policy version
    /// in a single call. Replaces inline consent record filtering in clients.
    pub fn get_consent_status(
        &self,
        consent_type: MobileConsentType,
    ) -> Result<MobileConsentStatus, MobileError> {
        let vauchi = self.open_vauchi()?;
        let status =
            vauchi.get_consent_status(vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(MobileConsentStatus::from(status))
    }

    /// Get all consent records.
    pub fn get_consent_records(&self) -> Result<Vec<MobileConsentRecord>, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        let records = manager
            .export_consent_log_with_version()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(records.iter().map(MobileConsentRecord::from).collect())
    }
}

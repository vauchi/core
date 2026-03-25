// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery operations for mobile.

use vauchi_core::recovery::{RecoveryClaim, RecoveryProof, RecoveryVoucher};

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{
    MobileRecoveryClaim, MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher,
};

#[uniffi::export]
impl VauchiPlatform {
    // === Recovery ===

    /// Create a recovery claim for a lost identity.
    ///
    /// The old_pk_hex is the hex-encoded public key of the lost identity.
    /// This starts the recovery process by creating a claim that contacts
    /// can vouch for.
    pub fn create_recovery_claim(
        &self,
        old_pk_hex: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        // Parse old public key
        let old_pk_bytes = hex::decode(&old_pk_hex)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid hex: {}", e)))?;
        let old_pk: [u8; 32] = old_pk_bytes
            .try_into()
            .map_err(|_| MobileError::InvalidInput("Public key must be 32 bytes".to_string()))?;

        // Create claim
        let new_pk = *identity.signing_public_key();
        let claim = RecoveryClaim::new(&old_pk, &new_pk);

        // Create proof to store vouchers and save to file
        let proof = RecoveryProof::new(&old_pk, &new_pk, 3); // Default threshold of 3
        std::fs::write(
            self.recovery_proof_path(),
            proof
                .to_bytes()
                .map_err(|e| MobileError::CryptoError(e.to_string()))?,
        )
        .map_err(|e| MobileError::StorageError(e.to_string()))?;

        // Encode claim for sharing
        let claim_data = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());

        Ok(MobileRecoveryClaim {
            old_public_key: old_pk_hex,
            new_public_key: hex::encode(new_pk),
            claim_data,
            is_expired: claim.is_expired(),
        })
    }

    /// Parse a recovery claim from base64.
    ///
    /// Used to inspect a claim before vouching for it.
    pub fn parse_recovery_claim(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        Ok(MobileRecoveryClaim {
            old_public_key: hex::encode(claim.old_pk()),
            new_public_key: hex::encode(claim.new_pk()),
            claim_data: claim_b64,
            is_expired: claim.is_expired(),
        })
    }

    /// Create a voucher for someone's recovery claim.
    ///
    /// This vouches that you trust the person claiming to own the old identity
    /// is the same person as the new identity.
    pub fn create_recovery_voucher(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryVoucher, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        if claim.is_expired() {
            return Err(MobileError::InvalidInput("Claim has expired".to_string()));
        }

        let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair())
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let voucher_data = base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

        Ok(MobileRecoveryVoucher {
            voucher_public_key: hex::encode(voucher.voucher_pk()),
            voucher_data,
        })
    }

    /// Add a voucher to the current recovery claim.
    ///
    /// Returns the updated progress.
    pub fn add_recovery_voucher(
        &self,
        voucher_b64: String,
    ) -> Result<MobileRecoveryProgress, MobileError> {
        use base64::Engine;
        let voucher_bytes = base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let voucher = RecoveryVoucher::from_bytes(&voucher_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid voucher: {}", e)))?;

        if !voucher.verify() {
            return Err(MobileError::InvalidInput(
                "Invalid voucher signature".to_string(),
            ));
        }

        // Load current proof from file
        let proof_path = self.recovery_proof_path();
        let mut proof = if proof_path.exists() {
            let proof_bytes =
                std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;
            RecoveryProof::from_bytes(&proof_bytes)
                .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?
        } else {
            return Err(MobileError::InvalidInput(
                "No recovery in progress".to_string(),
            ));
        };

        // Add voucher (enforce trusted contacts only)
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let trusted_keys: std::collections::HashSet<[u8; 32]> = contacts
            .iter()
            .filter(|c| c.is_recovery_trusted())
            .filter_map(|c| c.public_key().copied())
            .collect();

        match proof.add_voucher_trusted(voucher, &trusted_keys) {
            Ok(()) => {}
            Err(vauchi_core::recovery::RecoveryError::UntrustedVoucher) => {
                return Err(MobileError::InvalidInput(
                    "Voucher is from an untrusted contact. Only contacts marked as recovery-trusted can provide valid vouchers.".to_string(),
                ));
            }
            Err(e) => {
                return Err(MobileError::InvalidInput(format!(
                    "Cannot add voucher: {}",
                    e
                )));
            }
        }

        // Save updated proof
        std::fs::write(
            &proof_path,
            proof
                .to_bytes()
                .map_err(|e| MobileError::CryptoError(e.to_string()))?,
        )
        .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        })
    }

    /// Get the current recovery progress.
    ///
    /// Returns None if no recovery is in progress.
    pub fn get_recovery_status(&self) -> Result<Option<MobileRecoveryProgress>, MobileError> {
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(Some(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        }))
    }

    /// Get the completed recovery proof as base64.
    ///
    /// Returns None if recovery is not complete.
    pub fn get_recovery_proof(&self) -> Result<Option<String>, MobileError> {
        use base64::Engine;
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        if proof.voucher_count() >= proof.threshold() as usize {
            let proof_data = base64::engine::general_purpose::STANDARD.encode(
                proof
                    .to_bytes()
                    .map_err(|e| MobileError::CryptoError(e.to_string()))?,
            );
            Ok(Some(proof_data))
        } else {
            Ok(None)
        }
    }

    /// Verify a recovery proof from a contact.
    ///
    /// This checks if the proof is valid and provides a recommendation
    /// on whether to accept the recovered identity.
    pub fn verify_recovery_proof(
        &self,
        proof_b64: String,
    ) -> Result<MobileRecoveryVerification, MobileError> {
        use base64::Engine;
        let storage = self.open_storage()?;

        let proof_bytes = base64::engine::general_purpose::STANDARD
            .decode(&proof_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        // Validate the proof
        proof
            .validate()
            .map_err(|e| MobileError::InvalidInput(format!("Proof validation failed: {}", e)))?;

        // Count known vouchers (vouchers from our contacts)
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let contact_pks: std::collections::HashSet<[u8; 32]> = contacts
            .iter()
            .filter_map(|c| c.public_key().copied())
            .collect();

        let known_voucher_count = proof
            .vouchers()
            .iter()
            .filter(|v| contact_pks.contains(v.voucher_pk()))
            .count();

        // Determine confidence
        let (confidence, recommendation) = if known_voucher_count >= 2 {
            (
                "high".to_string(),
                "Multiple contacts you know have vouched. Safe to accept.".to_string(),
            )
        } else if known_voucher_count == 1 {
            (
                "medium".to_string(),
                "One contact you know has vouched. Consider verifying in person.".to_string(),
            )
        } else {
            (
                "low".to_string(),
                "No known contacts have vouched. Verify identity carefully before accepting."
                    .to_string(),
            )
        };

        Ok(MobileRecoveryVerification {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            voucher_count: proof.voucher_count() as u32,
            known_vouchers: known_voucher_count as u32,
            confidence,
            recommendation,
        })
    }
}

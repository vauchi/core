// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `recovery_backup` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_recovery_backup(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::VerifyRecoveryProof { proof_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryProof;

                let storage = engine.vauchi().storage();

                let proof_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&proof_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;
                proof.validate().map_err(|e| MobileError::InvalidInput {
                    field: String::new(),
                    detail: format!("Proof validation failed: {e}"),
                })?;

                let contacts =
                    storage
                        .contacts()
                        .list_contacts()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                let contact_pks: std::collections::HashSet<[u8; 32]> = contacts
                    .iter()
                    .filter_map(|c| c.public_key().copied())
                    .collect();
                let known_voucher_count = proof
                    .vouchers()
                    .iter()
                    .filter(|v| contact_pks.contains(v.voucher_pk().as_bytes()))
                    .count();

                let (confidence, recommendation) = if known_voucher_count >= 2 {
                    (
                        "high".to_string(),
                        "Multiple contacts you know have vouched. Safe to accept.".to_string(),
                    )
                } else if known_voucher_count == 1 {
                    (
                        "medium".to_string(),
                        "One contact you know has vouched. Consider verifying in person."
                            .to_string(),
                    )
                } else {
                    (
                        "low".to_string(),
                        "No known contacts have vouched. Verify identity carefully before accepting."
                            .to_string(),
                    )
                };

                Ok(DomainCommandResult::RecoveryVerification {
                    verification: crate::types::MobileRecoveryVerification {
                        old_public_key: hex::encode(proof.old_pk()),
                        new_public_key: hex::encode(proof.new_pk()),
                        voucher_count: proof.voucher_count() as u32,
                        known_vouchers: known_voucher_count as u32,
                        confidence,
                        recommendation,
                    },
                })
            }
            DomainCommand::UploadGuardianEntries => {
                engine
                    .vauchi()
                    .upload_guardian_entries()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                // Guardian entries don't directly drive any visible
                // screen — they're a network-side artefact. No cache
                // invalidation needed.
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SaveRecoveryResponse {
                claim_id,
                contact_id,
                response,
                remind_at,
            } => {
                engine
                    .vauchi()
                    .save_recovery_response_action(&claim_id, &contact_id, &response, remind_at)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::Unit)
            }

            // ── Recovery-trust toggle + count (slice 32g-B) ──
            DomainCommand::TrustContactForRecovery { contact_id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .contacts()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                if contact.is_blocked() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Blocked contacts cannot be trusted for recovery".into(),
                    });
                }
                contact
                    .trust_for_recovery()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage.contacts().save_contact(&contact).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UntrustContactForRecovery { contact_id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .contacts()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                contact
                    .untrust_for_recovery()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage.contacts().save_contact(&contact).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::TrustedContactCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .iter()
                    .filter(|c| c.is_recovery_trusted())
                    .count() as u32;
                Ok(DomainCommandResult::Count { value: count })
            }

            // ── Visibility Labels + Field Visibility (B7 batch 6) ──
            //
            // Cache invalidation: write-path commands invalidate the
            // Groups / GroupDetail / ContactDetail / ContactVisibility
            // screens. Reads invalidate nothing.
            DomainCommand::ExportBackup { password } => {
                let backup_hex =
                    engine
                        .vauchi()
                        .export_backup(&password)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                use base64::Engine;
                let bytes = hex::decode(&backup_hex).map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(DomainCommandResult::Text { value: encoded })
            }
            DomainCommand::ImportBackup {
                backup_data,
                password,
            } => {
                if engine.vauchi().identity().is_some() {
                    return Err(MobileError::Other {
                        detail: "Already initialized".to_string(),
                    });
                }
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&backup_data)
                    .map_err(|_| MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid base64".to_string(),
                    })?;
                let backup_hex = hex::encode(&bytes);
                engine
                    .vauchi_mut()
                    .import_backup(&backup_hex, &password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ExportFullBackup { password } => {
                let backup_hex = engine.vauchi().export_full_backup(&password).map_err(|e| {
                    MobileError::Other {
                        detail: e.to_string(),
                    }
                })?;
                use base64::Engine;
                let bytes = hex::decode(&backup_hex).map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(DomainCommandResult::Text { value: encoded })
            }
            DomainCommand::ImportFullBackup {
                backup_data,
                password,
            } => {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&backup_data)
                    .map_err(|_| MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid base64".to_string(),
                    })?;
                let backup_hex = hex::encode(&bytes);
                engine
                    .vauchi_mut()
                    .import_full_backup(&backup_hex, &password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ImportContactsFromVcf { data } => {
                let result = engine
                    .vauchi()
                    .import_contacts_from_vcf(&data)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::ImportResult {
                    result: crate::mobile_import::MobileImportResult {
                        imported: result.imported as u32,
                        skipped: result.skipped as u32,
                        warnings: result.warnings.into_iter().map(Into::into).collect(),
                    },
                })
            }

            // ── Search + Display Prefs + Merge (B7 batch 14) ──
            // SearchContacts arm already provided by batch 10 above.
            DomainCommand::ParseRecoveryClaim { claim_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryClaim;

                let claim_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&claim_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let claim = RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid claim: {e}"),
                    }
                })?;

                Ok(DomainCommandResult::RecoveryClaim {
                    claim: crate::types::MobileRecoveryClaim {
                        old_public_key: hex::encode(claim.old_pk()),
                        new_public_key: hex::encode(claim.new_pk()),
                        claim_data: claim_b64,
                        is_expired: claim
                            .is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
                    },
                })
            }
            DomainCommand::GetRecoveryProof => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryProof;

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Ok(DomainCommandResult::StringOpt { value: None });
                }

                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                let value = if proof.voucher_count() >= proof.threshold() as usize {
                    let bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
                } else {
                    None
                };
                Ok(DomainCommandResult::StringOpt { value })
            }
            DomainCommand::GetRecoveryStatus => {
                use vauchi_core::recovery::RecoveryProof;

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Ok(DomainCommandResult::OptionalRecoveryProgress { progress: None });
                }

                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                Ok(DomainCommandResult::OptionalRecoveryProgress {
                    progress: Some(crate::types::MobileRecoveryProgress {
                        old_public_key: hex::encode(proof.old_pk()),
                        new_public_key: hex::encode(proof.new_pk()),
                        vouchers_collected: proof.voucher_count() as u32,
                        vouchers_needed: proof.threshold(),
                        is_complete: proof.voucher_count() >= proof.threshold() as usize,
                    }),
                })
            }
            DomainCommand::CreateRecoveryVoucher { claim_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

                // `engine` is already locked by the dispatch entry point;
                // re-locking the non-reentrant mutex would deadlock.
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;

                let claim_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&claim_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let claim = RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid claim: {e}"),
                    }
                })?;

                if claim.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()) {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Claim has expired".into(),
                    });
                }

                let voucher =
                    RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair(), None, 0)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                let voucher_data =
                    base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

                Ok(DomainCommandResult::RecoveryVoucher {
                    voucher: crate::types::MobileRecoveryVoucher {
                        voucher_public_key: hex::encode(voucher.voucher_pk()),
                        voucher_data,
                    },
                })
            }
            DomainCommand::AddRecoveryVoucher { voucher_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryProof, RecoveryVoucher};

                // `engine` already locked by dispatch entry — do not re-lock.
                let voucher_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&voucher_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let voucher = RecoveryVoucher::from_bytes(&voucher_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid voucher: {e}"),
                    }
                })?;

                if !voucher.verify() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid voucher signature".into(),
                    });
                }

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "No recovery in progress".into(),
                    });
                }
                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let mut proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                let contacts = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let trusted_keys: std::collections::HashSet<[u8; 32]> = contacts
                    .iter()
                    .filter(|c| c.is_recovery_trusted())
                    .filter_map(|c| c.public_key().copied())
                    .collect();

                match proof.add_voucher_trusted(voucher, &trusted_keys) {
                    Ok(()) => {}
                    Err(vauchi_core::recovery::RecoveryError::UntrustedVoucher) => {
                        return Err(MobileError::InvalidInput {
                            field: String::new(),
                            detail: "Voucher is from an untrusted contact. Only contacts marked as recovery-trusted can provide valid vouchers.".into(),
                        });
                    }
                    Err(e) => {
                        return Err(MobileError::InvalidInput {
                            field: String::new(),
                            detail: format!("Cannot add voucher: {e}"),
                        });
                    }
                }

                let updated_bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                std::fs::write(&proof_path, updated_bytes).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;

                let progress = crate::types::MobileRecoveryProgress {
                    old_public_key: hex::encode(proof.old_pk()),
                    new_public_key: hex::encode(proof.new_pk()),
                    vouchers_collected: proof.voucher_count() as u32,
                    vouchers_needed: proof.threshold(),
                    is_complete: proof.voucher_count() >= proof.threshold() as usize,
                };

                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::RecoveryProgress { progress })
            }
            DomainCommand::CreateRecoveryClaim { old_pk_hex } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryClaim, RecoveryProof};

                // `engine` already locked by dispatch entry — do not re-lock.
                // Scope the identity borrow so the later `invalidate_screen`
                // mutable borrows are free.
                let new_pk = {
                    let identity =
                        engine
                            .vauchi()
                            .identity()
                            .ok_or_else(|| MobileError::Other {
                                detail: "Identity not initialized".into(),
                            })?;
                    *identity.signing_public_key()
                };

                let old_pk_bytes =
                    hex::decode(&old_pk_hex).map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid hex: {e}"),
                    })?;
                let old_pk: [u8; 32] =
                    old_pk_bytes
                        .try_into()
                        .map_err(|_| MobileError::InvalidInput {
                            field: String::new(),
                            detail: "Public key must be 32 bytes".into(),
                        })?;

                let now = vauchi_core::clock::SystemClock::shared().unix_seconds();
                let claim = RecoveryClaim::new(old_pk, new_pk, now);

                // Persist a `RecoveryProof` shell beside the database —
                // mirrors the legacy file layout. Threshold default 3.
                let proof = RecoveryProof::new(old_pk, new_pk, 3, now);
                let proof_bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                std::fs::write(self.recovery_proof_path(), proof_bytes).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;

                let claim_data = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());
                let result = crate::types::MobileRecoveryClaim {
                    old_public_key: old_pk_hex,
                    new_public_key: hex::encode(new_pk),
                    claim_data,
                    is_expired: claim.is_expired(now),
                };

                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::RecoveryClaim { claim: result })
            }
            other => unreachable!(
                "non-recovery_backup command {other:?} routed to recovery_backup dispatcher"
            ),
        }
    }
}

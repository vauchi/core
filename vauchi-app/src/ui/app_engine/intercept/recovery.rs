// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Social-recovery intercepts: create claim, verify claim, create voucher.
//! Split out of `intercept.rs` (cohesion). These wrap the crypto-bearing
//! `Vauchi` recovery API (ADR-002 — logic unchanged, moved verbatim).
//! `impl AppEngine` methods, dispatched from `dispatch.rs`.

use super::super::AppEngine;
use crate::ui::action::ActionResult;

/// A scanned hand-back: the share and the attempt token, base64 and joined by
/// a dot. Bounded before decoding because the camera's bytes are untrusted.
fn parse_scanned_hand_back(scanned: &str) -> Option<(Vec<u8>, [u8; 32])> {
    use base64::Engine as _;
    const MAX_SCANNED_BYTES: usize = 4096;
    if scanned.len() > MAX_SCANNED_BYTES {
        return None;
    }
    let codec = base64::engine::general_purpose::STANDARD;
    let (share, token) = scanned.trim().split_once('.')?;
    let share = codec.decode(share).ok()?;
    let token = codec.decode(token).ok()?.try_into().ok()?;
    Some((share, token))
}

impl AppEngine {
    /// Intercept the "create_claim" action on the Recovery screen
    /// (EnterOldKey step).
    ///
    /// Reads the engine's `old_key_input` (a hex-encoded public key),
    /// starts identity-free recovery or uses the existing identity's claim,
    /// then either
    /// advances the engine to ShowGeneratedClaim (success) or attaches
    /// a validation error to the input (failure). Returns `UpdateScreen`
    /// so the rendered screen reflects the new engine state.
    pub(in crate::ui::app_engine) fn intercept_create_claim_action(
        &mut self,
    ) -> Option<ActionResult> {
        let old_key = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Recovery { old_key_input, .. }) => {
                old_key_input.trim().to_string()
            }
            _ => return None,
        };

        let result = if self.vauchi.has_identity() {
            self.vauchi.create_recovery_claim_hex_b64(&old_key)
        } else {
            self.vauchi.begin_recovery(&old_key)
        };

        let update = match result {
            Ok(claim_b64) => crate::ui::RecoveryUpdate::ClaimGenerated(claim_b64),
            Err(e) => crate::ui::RecoveryUpdate::ClaimCreateError(format!("{e}")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// A refusal looks the same for every cause (A11), so the alert says only
    /// that nothing can be handed back.
    pub(in crate::ui::app_engine) fn intercept_hand_back_action(&mut self) -> ActionResult {
        use base64::Engine as _;
        let claim_input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::RecoveryHelp { claim_input }) => {
                claim_input.trim().to_string()
            }
            _ => String::new(),
        };
        match self.vauchi.hand_back_in_person(&claim_input) {
            Ok(hand_back) => {
                let codec = base64::engine::general_purpose::STANDARD;
                let Some(token) = self.begin_hand_back_link(hand_back.backup_pieces()) else {
                    return ActionResult::ShowAlert {
                        title: self.t("recovery.hand_back_title"),
                        message: self.t("recovery.hand_back_refused"),
                    };
                };
                let share = format!(
                    "{}.{}",
                    codec.encode(hand_back.share()),
                    codec.encode(token)
                );
                self.engine
                    .apply_update(crate::ui::EngineUpdate::RecoveryHelp(
                        crate::ui::RecoveryHelpUpdate::HandBackReady(share),
                    ));
                ActionResult::UpdateScreen(self.engine.current_screen())
            }
            Err(_) => ActionResult::ShowAlert {
                title: self.t("recovery.hand_back_title"),
                message: self.t("recovery.hand_back_refused"),
            },
        }
    }

    /// Takes a guardian's share scanned on the identity-free phone. Bytes
    /// from the camera are untrusted: `take_hand_back` parses and opens them
    /// or refuses, and a refusal changes nothing.
    pub(in crate::ui::app_engine) fn intercept_take_hand_back(
        &mut self,
        scanned: &str,
    ) -> ActionResult {
        let Some((share, token)) = parse_scanned_hand_back(scanned) else {
            return self.share_refused();
        };
        let Ok(share_taken) = self.vauchi.take_hand_back(&share) else {
            return self.share_refused();
        };
        let backup_hash = vauchi_core::sync::guardian_piece::HandBackShare::decode(&share)
            .map(|decoded| decoded.backup_hash());
        let whole = backup_hash.as_ref().is_ok_and(|hash| {
            self.vauchi
                .storage()
                .recovery()
                .backup_piece_progress(hash)
                .ok()
                .flatten()
                .is_some_and(|(received, count)| received >= count)
        });
        if whole {
            self.offer_restore_when_ready();
        } else if let Ok(hash) = backup_hash {
            self.begin_share_link(&token, hash);
        }
        let taken = Some(share_taken);
        match taken {
            Some(1) => ActionResult::ShowToast {
                message: self.t("recovery.restore_handed_back_one"),
                undo_action_id: None,
                undo_label: None,
            },
            Some(count) => ActionResult::ShowToast {
                message: crate::i18n::get_string_with_args(
                    self.render_context.resolved_locale(),
                    "recovery.restore_handed_back",
                    &[("count", &count.to_string())],
                ),
                undo_action_id: None,
                undo_label: None,
            },
            None => self.share_refused(),
        }
    }

    fn share_refused(&self) -> ActionResult {
        ActionResult::ShowAlert {
            title: self.t("recovery.hand_back_title"),
            message: self.t("recovery.hand_back_share_refused"),
        }
    }

    /// Renewal is the one act that extends a claim's lifetime, so a refusal
    /// is shown rather than falling back to the expired claim.
    pub(in crate::ui::app_engine) fn intercept_renew_claim_action(&mut self) -> ActionResult {
        let update = match self.vauchi.renew_pending_recovery_claim() {
            Ok(claim_b64) => crate::ui::RecoveryUpdate::ClaimGenerated(claim_b64),
            Err(_) => crate::ui::RecoveryUpdate::ClaimRenewalFailed,
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update));
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    /// Intercept the "verify_claim" action on the RecoveryHelp screen.
    ///
    /// Reads the user-pasted claim payload from the engine, base64-decodes
    /// and parses it via `RecoveryClaim::from_bytes`, then either advances
    /// the engine to the ConfirmVoucher step (success) or attaches a
    /// validation error to the input (failure). Returns `UpdateScreen` so
    /// the rendered screen reflects the new engine state.
    pub(in crate::ui::app_engine) fn intercept_verify_claim_action(
        &mut self,
    ) -> Option<ActionResult> {
        let claim_input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::RecoveryHelp { claim_input }) => {
                claim_input.trim().to_string()
            }
            _ => return None,
        };

        let parse_result = self.vauchi.parse_recovery_claim_b64(&claim_input);

        let update = match parse_result
            .and_then(|claim| Ok((self.vauchi.recovery_claim_contact(&claim)?, claim)))
        {
            Ok((contact, claim)) => crate::ui::RecoveryHelpUpdate::ParsedClaim(
                crate::ui::recovery_help::ParsedClaimSummary {
                    contact_name: contact.display_name().to_string(),
                    old_pk_hex: hex::encode::<&[u8]>(claim.old_pk().as_ref()),
                    new_pk_hex: hex::encode::<&[u8]>(claim.new_pk().as_ref()),
                    is_expired: claim.is_expired(self.vauchi.clock().unix_seconds()),
                    keeps_backup: self.vauchi.keeps_backup_of(&claim).unwrap_or(false),
                },
            ),
            Err(vauchi_core::VauchiError::ContactNotFound(_)) => {
                crate::ui::RecoveryHelpUpdate::ClaimParseError(
                    self.t("recovery.claim_not_a_contact"),
                )
            }
            Err(e) => crate::ui::RecoveryHelpUpdate::ClaimParseError(format!("Invalid claim: {e}")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::RecoveryHelp(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the "create_voucher" action on the RecoveryHelp screen.
    ///
    /// Re-decodes the claim from the engine input and signs a voucher with
    /// the local identity's signing keypair via
    /// `Vauchi::create_voucher_from_claim_b64` (mirrors the existing
    /// mobile platform `create_recovery_voucher` flow — no guardian
    /// token, no relay round-trip). Stores the base64 voucher payload on
    /// the engine so the ShowVoucher screen can render it for the user
    /// to share.
    pub(in crate::ui::app_engine) fn intercept_create_voucher_action(
        &mut self,
    ) -> Option<ActionResult> {
        let claim_input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::RecoveryHelp { claim_input }) => {
                claim_input.trim().to_string()
            }
            _ => return None,
        };

        match self.vauchi.create_voucher_from_claim_b64(&claim_input) {
            Ok(voucher_b64) => self
                .engine
                .apply_update(crate::ui::EngineUpdate::RecoveryHelp(
                    crate::ui::RecoveryHelpUpdate::VoucherData(voucher_b64),
                ))
                .then(|| ActionResult::UpdateScreen(self.engine.current_screen())),
            Err(e) => Some(ActionResult::ShowAlert {
                title: self.t("recovery_help.voucher_error_title"),
                message: format!("{e}"),
            }),
        }
    }

    /// Intercept "add_voucher" on the Recovery screen (CollectVouchers
    /// step): takes the pasted voucher into the stored proof through
    /// `Vauchi::add_recovery_voucher_b64`, whose checks decide (other
    /// claim, self-vouching, signature, duplicate signer), and shows the
    /// proof's signers or why the voucher was refused (private#877).
    pub(in crate::ui::app_engine) fn intercept_add_voucher_action(
        &mut self,
    ) -> Option<ActionResult> {
        let voucher = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Recovery {
                voucher_input: Some(voucher),
                ..
            }) => voucher,
            _ => return None,
        };

        let update = match self.vauchi.add_recovery_voucher_b64(&voucher) {
            Ok(progress) => vouchers_collected(&progress),
            Err(_) => {
                crate::ui::RecoveryUpdate::VoucherRefused(self.t("recovery.voucher_not_accepted"))
            }
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept "submit_proof" on the Recovery screen (CollectVouchers
    /// step): uploads the proof to the relay, and shows Complete only once
    /// the relay took it. Until private#877 the engine showed Complete
    /// and nothing was sent.
    pub(in crate::ui::app_engine) fn intercept_submit_proof_action(
        &mut self,
    ) -> Option<ActionResult> {
        if !matches!(
            self.engine.engine_output(),
            Some(crate::ui::EngineOutput::Recovery {
                voucher_input: Some(_),
                ..
            })
        ) {
            return None;
        }

        #[cfg(feature = "network-http")]
        let uploaded = self.vauchi.upload_recovery_proof();
        // Without a relay transport there is nowhere to send the proof.
        #[cfg(not(feature = "network-http"))]
        let uploaded: Result<(), ()> = Err(());
        match uploaded {
            Ok(()) => self
                .engine
                .apply_update(crate::ui::EngineUpdate::Recovery(
                    crate::ui::RecoveryUpdate::ProofSubmitted,
                ))
                .then(|| ActionResult::UpdateScreen(self.engine.current_screen())),
            Err(_) => Some(ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: self.t("recovery.proof_upload_failed"),
            }),
        }
    }
}

impl AppEngine {
    /// Intercept saving or removing the owner's recovery passphrase on
    /// the Recovery screen (D16): the two entries must match, and Core
    /// judges the strength and sends the guardians a new backup.
    pub(in crate::ui::app_engine) fn intercept_passphrase_action(
        &mut self,
        remove: bool,
    ) -> Option<ActionResult> {
        let Some(crate::ui::EngineOutput::Recovery {
            new_passphrase: Some((new, confirm)),
            ..
        }) = self.engine.engine_output()
        else {
            return None;
        };
        let update = if remove {
            match self.vauchi.clear_recovery_passphrase() {
                Ok(()) => crate::ui::RecoveryUpdate::PassphraseSet(false),
                Err(_) => crate::ui::RecoveryUpdate::PassphraseRefused(
                    self.t("recovery.passphrase_not_saved"),
                ),
            }
        } else if new != confirm {
            crate::ui::RecoveryUpdate::PassphraseRefused(self.t("recovery.passphrase_mismatch"))
        } else {
            match self.vauchi.set_recovery_passphrase(&new) {
                Ok(()) => crate::ui::RecoveryUpdate::PassphraseSet(true),
                Err(_) => {
                    crate::ui::RecoveryUpdate::PassphraseRefused(self.t("recovery.passphrase_weak"))
                }
            }
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept "restore_now" on the Recovery screen's restore step:
    /// restores what the guardians handed back (vauchi/private#796, step
    /// 3b), with the typed passphrase when the backup needs one.
    pub(in crate::ui::app_engine) fn intercept_restore_action(&mut self) -> Option<ActionResult> {
        let Some(crate::ui::EngineOutput::Recovery {
            restore_passphrase: Some(passphrase),
            restore_same_identity,
            ..
        }) = self.engine.engine_output()
        else {
            return None;
        };
        let passphrase = (!passphrase.is_empty()).then_some(passphrase);
        let restored = if restore_same_identity {
            self.vauchi
                .restore_identity_from_hand_backs(passphrase.as_deref())
        } else {
            self.vauchi
                .restore_from_returned_backups(passphrase.as_deref())
        };
        let update = match restored {
            Ok(count) => crate::ui::RecoveryUpdate::Restored(count),
            Err(_) => crate::ui::RecoveryUpdate::RestoreRefused(self.t("recovery.restore_refused")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}

/// The first four groups of a key's fingerprint, in the format of
/// `Vauchi::own_fingerprint`: enough to tell helpers apart on screen.
/// The proof's signers and threshold, as the recovery screen lists them.
pub(in crate::ui::app_engine) fn vouchers_collected(
    progress: &vauchi_core::RecoveryProgress,
) -> crate::ui::RecoveryUpdate {
    crate::ui::RecoveryUpdate::VouchersCollected {
        signers: progress
            .vouchers
            .iter()
            .map(|voucher| short_fingerprint(voucher.voucher_pk().as_bytes()))
            .collect(),
        threshold: progress.threshold as usize,
    }
}

fn short_fingerprint(public_key: &[u8; 32]) -> String {
    hex::encode_upper(&public_key[..8])
        .as_bytes()
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

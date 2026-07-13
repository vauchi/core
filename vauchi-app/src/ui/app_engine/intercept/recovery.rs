// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social-recovery intercepts: create claim, verify claim, create voucher.
//! Split out of `intercept.rs` (cohesion). These wrap the crypto-bearing
//! `Vauchi` recovery API (ADR-002 — logic unchanged, moved verbatim).
//! `impl AppEngine` methods, dispatched from `dispatch.rs`.

use super::super::AppEngine;
use crate::ui::action::ActionResult;

impl AppEngine {
    /// Intercept the "create_claim" action on the Recovery screen
    /// (EnterOldKey step).
    ///
    /// Reads the engine's `old_key_input` (a hex-encoded public key),
    /// passes it to `Vauchi::create_recovery_claim_hex_b64`, then either
    /// advances the engine to ShowGeneratedClaim (success) or attaches
    /// a validation error to the input (failure). Returns `UpdateScreen`
    /// so the rendered screen reflects the new engine state.
    pub(in crate::ui::app_engine) fn intercept_create_claim_action(
        &mut self,
    ) -> Option<ActionResult> {
        let old_key = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Recovery { old_key_input }) => {
                old_key_input.trim().to_string()
            }
            _ => return None,
        };

        let result = self.vauchi.create_recovery_claim_hex_b64(&old_key);

        let update = match result {
            Ok(claim_b64) => crate::ui::RecoveryUpdate::ClaimGenerated(claim_b64),
            Err(e) => crate::ui::RecoveryUpdate::ClaimCreateError(format!("{e}")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
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

        let update = match parse_result {
            Ok(claim) => crate::ui::RecoveryHelpUpdate::ParsedClaim(
                crate::ui::recovery_help::ParsedClaimSummary {
                    old_pk_hex: hex::encode::<&[u8]>(claim.old_pk().as_ref()),
                    new_pk_hex: hex::encode::<&[u8]>(claim.new_pk().as_ref()),
                    is_expired: claim.is_expired(self.vauchi.clock().unix_seconds()),
                },
            ),
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
}

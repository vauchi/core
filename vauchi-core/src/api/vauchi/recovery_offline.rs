// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery operations that do not need relay access.
//!
//! These helpers parse claim payloads and sign vouchers locally — they
//! don't touch the relay and are safe to compile without the
//! `network-http` feature. Used by the app-layer `RecoveryHelpEngine` so
//! helper-side UI works on platforms that build without relay support.

use base64::Engine;

use crate::api::error::{VauchiError, VauchiResult};
use crate::recovery::{RecoveryClaim, RecoveryVoucher};

use super::Vauchi;

impl Vauchi {
    /// Parses a base64-encoded recovery claim received from a contact who
    /// is recovering their identity.
    ///
    /// Used by the helper-side recovery UI — the recovering user shares
    /// their claim payload (string) and the helper decodes + verifies it
    /// before signing a voucher with `create_voucher_from_claim_b64`.
    pub fn parse_recovery_claim_b64(&self, claim_b64: &str) -> VauchiResult<RecoveryClaim> {
        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(claim_b64.trim())
            .map_err(|e| VauchiError::Serialization(format!("invalid base64: {e}")))?;
        RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))
    }

    /// Signs a recovery voucher for a base64-encoded claim, using the local
    /// identity's signing keypair, and returns the base64-encoded voucher
    /// payload ready for the recovering user to add to their proof.
    ///
    /// Mirrors the existing platform-layer `create_recovery_voucher`
    /// helper, but exposed at the Vauchi API level so the app-layer UI
    /// engines can use it without reaching into Identity directly.
    /// No guardian token is attached (matches the current mobile flow).
    pub fn create_voucher_from_claim_b64(&self, claim_b64: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let claim = self.parse_recovery_claim_b64(claim_b64)?;
        let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair(), None)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes()))
    }
}

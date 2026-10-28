// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery operations that do not need relay access.
//!
//! These helpers parse claim payloads and sign vouchers locally — they
//! don't touch the relay and are safe to compile without the
//! `network-http` feature. Used by the app-layer `RecoveryHelpEngine` so
//! helper-side UI works on platforms that build without relay support.

use base64::Engine;

use crate::api::error::{VauchiError, VauchiResult};
use crate::contact::Contact;
use crate::recovery::{RecoveryClaim, RecoveryProgress, RecoveryVoucher};

use super::Vauchi;

impl Vauchi {
    /// Creates a claim binding `old_pk` to this identity and stores it as
    /// the recovery in progress, so vouchers have a claim to attach to.
    pub(super) fn start_recovery_claim(&self, old_pk: &[u8; 32]) -> VauchiResult<RecoveryClaim> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let now = self.clock.unix_seconds();
        let claim = RecoveryClaim::new(old_pk, identity.signing_public_key(), now);

        let settings = self
            .storage
            .recovery()
            .load_recovery_settings()?
            .unwrap_or_default();

        let progress = RecoveryProgress::new(claim.clone(), settings.recovery_threshold(), now);
        self.storage.recovery().save_recovery_progress(&progress)?;

        Ok(claim)
    }

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

    /// The contact whose key a recovery claim says was lost. Vouching for
    /// anyone else would sign for a key the helper never met, and the
    /// helper checks the person in front of them against this contact
    /// (vauchi/private#708).
    pub fn recovery_claim_contact(&self, claim: &RecoveryClaim) -> VauchiResult<Contact> {
        let old_pk = claim.old_pk().as_bytes();
        self.storage
            .contacts()
            .list_contacts()?
            .into_iter()
            .find(|contact| contact.public_key() == Some(old_pk))
            .ok_or_else(|| VauchiError::ContactNotFound(hex::encode(old_pk)))
    }

    /// Creates a recovery claim for the given old (lost) identity public
    /// key (hex-encoded), stores it as the recovery in progress, and returns
    /// the base64-encoded claim ready to share with trusted contacts.
    /// Mirrors the existing platform-layer `create_recovery_claim` helper,
    /// but exposed at the Vauchi API level so the app-layer UI engine can
    /// use it without reaching into Identity directly.
    ///
    /// Used by the Recover-tab UI on the recovering user's new device:
    /// the user pastes their previous identity's public key, this signs
    /// and serializes a claim, then guardians scan/paste the claim and
    /// vouch via `create_voucher_from_claim_b64`.
    ///
    /// Unlike `Vauchi::create_recovery_claim` (network-http gated, also
    /// persists `RecoveryProgress` for voucher collection tracking), this
    /// helper only creates + serializes the claim — no storage write,
    /// no relay round-trip. Suitable for the offline initial-claim
    /// share-with-guardians flow.
    pub fn create_recovery_claim_hex_b64(&self, old_pk_hex: &str) -> VauchiResult<String> {
        let old_pk_bytes = hex::decode(old_pk_hex.trim())
            .map_err(|e| VauchiError::Serialization(format!("invalid hex: {e}")))?;
        let old_pk: [u8; 32] = old_pk_bytes
            .try_into()
            .map_err(|_| VauchiError::Serialization("public key must be 32 bytes".into()))?;

        let claim = self.start_recovery_claim(&old_pk)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(claim.to_bytes()))
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
        self.recovery_claim_contact(&claim)?;
        let voucher = RecoveryVoucher::create_from_claim(
            &claim,
            identity.signing_keypair(),
            None,
            self.clock.unix_seconds(),
        )
        .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes()))
    }
}

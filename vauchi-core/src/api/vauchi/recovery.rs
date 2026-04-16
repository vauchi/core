// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery API methods for social recovery via guardian vouching.
//!
//! Provides guardian entry management (upload to relay) and recovery
//! flow operations (create claim, collect vouchers, submit proof).

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::{HKDF, X3DHKeyPair};
use crate::network::{HttpTransport, HttpTransportConfig};
use crate::recovery::guardian::GuardianToken;
use crate::recovery::sealed_box;
use crate::recovery::{RecoveryClaim, RecoveryProgress, RecoveryVoucher};
use vauchi_protocol::v2::V2GuardianEntry;

use super::Vauchi;

impl Vauchi {
    // === Guardian Management ===

    /// Uploads encrypted guardian entries to the relay.
    ///
    /// Creates a `GuardianToken` for each recovery-trusted contact,
    /// encrypts it to the guardian's X25519 key (derived from their Ed25519 key),
    /// and atomically replaces the full set on the relay.
    ///
    /// Called after `toggle_recovery_trust()` changes the guardian set.
    pub fn upload_guardian_entries(&self) -> VauchiResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.list_contacts()?;
        let guardians: Vec<_> = contacts
            .iter()
            .filter(|c| c.is_recovery_trusted())
            .collect();

        // Compute storage key: SHA-256(designator_pk || "guardians")
        let guardian_hash = compute_guardian_hash(identity.signing_public_key());

        if guardians.is_empty() {
            // No guardians — delete any existing entries on relay
            let transport = self.create_guardian_transport();
            transport.guardian_delete(&guardian_hash)?;
            return Ok(());
        }

        // Create and encrypt guardian tokens
        let mut entries = Vec::with_capacity(guardians.len());
        for guardian in &guardians {
            let guardian_ed25519_pk = guardian.public_key().ok_or_else(|| {
                VauchiError::InvalidState(format!("Guardian {} has no public key", guardian.id()))
            })?;

            // Create guardian token (signed by designator)
            let guardian_crypto_pk = crate::crypto::PublicKey::from_bytes(*guardian_ed25519_pk);
            let token = GuardianToken::create(identity.signing_keypair(), guardian_crypto_pk);
            let token_bytes = token.to_bytes();

            // Convert guardian's Ed25519 public key to X25519 for sealed-box
            let x25519_pk = ed25519_pk_to_x25519(guardian_ed25519_pk)?;

            // Encrypt token to guardian's X25519 key
            let sealed = sealed_box::seal(&token_bytes, &x25519_pk);

            entries.push(V2GuardianEntry {
                data: base64::engine::general_purpose::STANDARD.encode(&sealed),
            });
        }

        // Upload to relay
        let transport = self.create_guardian_transport();
        transport.guardian_store(&guardian_hash, entries)?;

        Ok(())
    }

    // === Outgoing Recovery (I'm recovering) ===

    /// Creates a recovery claim binding old_pk and new_pk.
    ///
    /// The claim binds the old (lost) identity to the new identity.
    /// old_pk is the identity we're trying to recover. Returns the claim
    /// and starts tracking recovery progress.
    pub fn create_recovery_claim(&self, old_pk: &[u8; 32]) -> VauchiResult<RecoveryClaim> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let claim = RecoveryClaim::new(old_pk, identity.signing_public_key());

        // Load recovery settings for threshold
        let settings = self.storage.load_recovery_settings()?.unwrap_or_default();

        let progress = RecoveryProgress::new(claim.clone(), settings.recovery_threshold());
        self.storage.save_recovery_progress(&progress)?;

        Ok(claim)
    }

    /// Adds a voucher to the in-progress recovery proof.
    ///
    /// Returns updated progress. The voucher is typically scanned from
    /// a guardian's QR code.
    pub fn add_recovery_voucher(&self, voucher_bytes: &[u8]) -> VauchiResult<RecoveryProgress> {
        let voucher = RecoveryVoucher::from_bytes(voucher_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        let mut progress = self
            .storage
            .load_recovery_progress()?
            .ok_or_else(|| VauchiError::InvalidState("No recovery in progress".into()))?;

        progress.add_voucher(voucher);
        self.storage.save_recovery_progress(&progress)?;

        Ok(progress)
    }

    /// Returns current recovery progress, if any.
    pub fn get_recovery_progress(&self) -> VauchiResult<Option<RecoveryProgress>> {
        Ok(self.storage.load_recovery_progress()?)
    }

    /// Uploads the completed recovery proof to the relay.
    ///
    /// The proof contains the claim + all collected vouchers. It's stored
    /// on the relay for the old identity's contacts to verify and accept.
    ///
    /// TODO: Requires a `recovery_store` method on `HttpTransport` — not
    /// yet implemented. Will be wired in a later phase when the relay
    /// recovery-store endpoint client method is added.
    pub fn upload_recovery_proof(&self) -> VauchiResult<()> {
        let progress = self
            .storage
            .load_recovery_progress()?
            .ok_or_else(|| VauchiError::InvalidState("No recovery in progress".into()))?;

        if !progress.is_complete() {
            return Err(VauchiError::InvalidState(format!(
                "Insufficient vouchers: have {}, need {}",
                progress.voucher_count(),
                progress.threshold,
            )));
        }

        // TODO: relay upload requires HttpTransport::recovery_store (not yet implemented)
        Err(VauchiError::InvalidState(
            "Not yet implemented: relay upload of recovery proof".into(),
        ))
    }

    // === Incoming Recovery (I'm helping / vouching) ===

    /// Queries the relay for guardian entries and creates a voucher for a claim.
    ///
    /// Looks up the guardian entries for the claim's old_pk, finds and
    /// decrypts our entry using our X25519 secret key, and creates a
    /// signed voucher with the guardian token.
    pub fn vouch_for_claim(
        &self,
        claim: &RecoveryClaim,
        contact_id: &str,
    ) -> VauchiResult<RecoveryVoucher> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Verify the contact exists
        let _contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;

        // Query relay for guardian entries
        let guardian_hash = compute_guardian_hash(claim.old_pk());
        let transport = self.create_guardian_transport();
        let entries = transport.guardian_query(&guardian_hash)?;

        if entries.is_empty() {
            return Err(VauchiError::InvalidState(
                "No guardian entries found on relay".into(),
            ));
        }

        // Re-derive our X25519 secret key from master seed (same path as Identity)
        let exchange_seed =
            HKDF::derive_key(None, identity.master_seed(), b"Vauchi_Exchange_Seed_v2");
        let x3dh = X3DHKeyPair::from_bytes(*exchange_seed);
        let our_x25519_secret = x25519_dalek::StaticSecret::from(*x3dh.secret_bytes());

        // Try to decrypt each entry with our X25519 secret key
        let mut found_token = None;
        for entry in &entries {
            let sealed_bytes = base64::engine::general_purpose::STANDARD
                .decode(&entry.data)
                .map_err(|_| {
                    VauchiError::Serialization("invalid base64 in guardian entry".into())
                })?;

            if let Ok(token_bytes) = sealed_box::open(&sealed_bytes, &our_x25519_secret)
                && let Ok(token) = GuardianToken::from_bytes(&token_bytes)
                && token.verify()
            {
                found_token = Some(token);
                break;
            }
            // Entry not for us — try next
        }

        let _token = found_token.ok_or_else(|| {
            VauchiError::InvalidState(
                "No guardian entry found for our key — we may not be a designated guardian".into(),
            )
        })?;

        // Create voucher: signs (old_pk, new_pk) with our keypair
        let voucher =
            RecoveryVoucher::create(claim.old_pk(), claim.new_pk(), identity.signing_keypair());

        Ok(voucher)
    }

    /// Saves a recovery response (accept, reject, or remind_me_later).
    pub fn save_recovery_response_action(
        &self,
        claim_id: &str,
        contact_id: &str,
        response: &str,
        remind_at: Option<u64>,
    ) -> VauchiResult<()> {
        self.storage
            .save_recovery_response(claim_id, contact_id, response, remind_at)?;
        Ok(())
    }

    // === Internal Helpers ===

    /// Creates an `HttpTransport` for guardian relay operations.
    ///
    /// Uses direct mode (no OHTTP) for upload operations. Query operations
    /// should use OHTTP for privacy, but that's wired in a later phase.
    fn create_guardian_transport(&self) -> HttpTransport {
        HttpTransport::new(HttpTransportConfig {
            relay_url: self.http_relay_url(),
            timeout_ms: self.config.relay.connect_timeout_ms,
            proxy: self.config.relay.proxy.clone(),
            allow_direct: true,
            pinned_certs: self.config.relay.pinned_certs.clone(),
        })
    }
}

/// Computes the guardian storage hash: SHA-256(designator_pk || "guardians").
///
/// Returns the hex-encoded hash (64 chars).
fn compute_guardian_hash(designator_pk: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(designator_pk);
    hasher.update(b"guardians");
    hex::encode(hasher.finalize())
}

/// Converts an Ed25519 public key to an X25519 (Curve25519) public key.
///
/// Uses the birational map from Edwards to Montgomery form.
fn ed25519_pk_to_x25519(ed25519_pk: &[u8; 32]) -> VauchiResult<x25519_dalek::PublicKey> {
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(ed25519_pk)
        .map_err(|e| VauchiError::Crypto(format!("invalid Ed25519 public key: {e}")))?;
    let montgomery = verifying_key.to_montgomery();
    Ok(x25519_dalek::PublicKey::from(montgomery.to_bytes()))
}

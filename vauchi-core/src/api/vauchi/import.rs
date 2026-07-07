// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact import API — wraps the vCard parser and persists imported contacts.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ImportSource;
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::contact_card::vcard_import::import_vcf;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// Reason a single vCard entry was skipped (G6 of the pure-renderer
/// remediation — ADR tracks at `_private/docs/problems/2026-04-16-frontend-pure-renderer-violations/`).
///
/// Each variant carries the data frontends need to render a localized
/// message; the `Display` impl produces the English rendering used by
/// CLI + tests + unlocalized fallback paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportWarning {
    /// A vCard with this UID already exists and was skipped (W7).
    DuplicateUid { uid: String },
    /// The per-identity contact limit was reached; remaining vCards
    /// were not imported. `max` is the configured limit (C3).
    ContactLimitReached { max: usize },
    /// Storage rejected the contact (disk full, schema failure, etc.).
    SaveFailed { error: String },
}

impl ImportWarning {
    /// Stable i18n key frontends look up via their `t()` helper.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            ImportWarning::DuplicateUid { .. } => "import.warning.duplicate_uid",
            ImportWarning::ContactLimitReached { .. } => "import.warning.limit_reached",
            ImportWarning::SaveFailed { .. } => "import.warning.save_failed",
        }
    }

    /// Placeholder values for the i18n template (e.g. `{"uid" => "abc"}`).
    pub fn args(&self) -> Vec<(String, String)> {
        match self {
            ImportWarning::DuplicateUid { uid } => vec![("uid".into(), uid.clone())],
            ImportWarning::ContactLimitReached { max } => vec![("max".into(), max.to_string())],
            ImportWarning::SaveFailed { error } => vec![("error".into(), error.clone())],
        }
    }
}

impl fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportWarning::DuplicateUid { uid } => write!(f, "Skipped duplicate (UID: {uid})"),
            ImportWarning::ContactLimitReached { max } => write!(
                f,
                "Contact limit reached ({max}); skipped remaining imports"
            ),
            ImportWarning::SaveFailed { error } => write!(f, "Skipped contact: {error}"),
        }
    }
}

/// Result of a contact import operation.
pub struct ImportResult {
    /// Number of contacts successfully imported.
    pub imported: usize,
    /// Number of contacts skipped (malformed or duplicate).
    pub skipped: usize,
    /// Structured per-contact warnings. Call `.to_string()` / `{}` for
    /// the English rendering, or read `i18n_key()` + `args()` for
    /// localized display.
    pub warnings: Vec<ImportWarning>,
}

impl Vauchi {
    /// Imports contacts from vCard data (supports 2.1 / 3.0 / 4.0, multi-contact files).
    ///
    /// Each successfully parsed vCard becomes an imported [`Contact`] stored
    /// with [`ImportSource::VcardFile`].  Cards that fail to save (e.g. because
    /// storage is full) are counted as skipped with a warning message.
    ///
    /// Returns an [`ImportResult`] with counts and any per-contact warnings.
    pub fn import_contacts_from_vcf(&self, data: &[u8]) -> VauchiResult<ImportResult> {
        let now = self.clock.unix_seconds();
        let entries =
            import_vcf(data, now).map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        let mut imported = 0;
        let mut skipped = 0;
        let mut warnings = Vec::new();

        // C3: Enforce contact limit — compute remaining budget once before the loop.
        let current_count = self.storage.contacts().count_contacts()?;
        let max_contacts = self.storage.contacts().get_contact_limit()?;
        let mut remaining_budget = max_contacts.saturating_sub(current_count);

        for (card, uid) in entries {
            // W7: Skip if a contact with this original_uid already exists.
            if let Some(ref uid_val) = uid
                && self
                    .storage
                    .contacts()
                    .find_imported_by_uid(uid_val)?
                    .is_some()
            {
                skipped += 1;
                warnings.push(ImportWarning::DuplicateUid {
                    uid: uid_val.clone(),
                });
                continue;
            }

            // C3: Stop importing when budget exhausted.
            if remaining_budget == 0 {
                skipped += 1;
                warnings.push(ImportWarning::ContactLimitReached { max: max_contacts });
                // Count the rest as skipped without iterating one-by-one.
                // We already incremented skipped for this entry; the loop
                // will naturally stop producing more warnings.
                continue;
            }

            let id = uuid::Uuid::new_v4().to_string();
            let contact = Contact::from_import(id, card, ImportSource::VcardFile, uid, 0);
            match self.storage.contacts().save_contact(&contact) {
                Ok(_) => {
                    imported += 1;
                    remaining_budget = remaining_budget.saturating_sub(1);
                }
                Err(e) => {
                    warnings.push(ImportWarning::SaveFailed {
                        error: e.to_string(),
                    });
                    skipped += 1;
                }
            }
        }

        Ok(ImportResult {
            imported,
            skipped,
            warnings,
        })
    }

    /// Save a contact card received via a **legacy v1** link-mode payload,
    /// returning the stored contact id.
    ///
    /// A v1 payload swaps the card over an ephemeral escrow key and
    /// establishes no persistent update channel (no shared comms key,
    /// no relay routing), so the result is an **imported** contact
    /// ([`ImportSource::LinkExchange`]) — not exchanged: there is no
    /// `shared_key`/relay to populate `ExchangedData` (HR-1). Idempotent:
    /// re-receiving the same card (matched by its card id) returns the
    /// existing contact id without duplicating.
    ///
    /// Since ADR-050 (T5b) this is the **fallback** path:
    /// [`Self::complete_link_exchange`] dispatches v2 bootstraps to a live
    /// `Exchanged` Link contact and only routes v1 payloads here. Slice
    /// `2026-05-24-core-exchange-completion-contact-save`.
    pub fn import_received_link_card(&self, card: ContactCard) -> VauchiResult<String> {
        let uid = card.id().to_string();
        // Idempotent dedup: the card id is the key.
        if let Some(existing_id) = self.storage.contacts().find_imported_by_uid(&uid)? {
            return Ok(existing_id);
        }
        // C3 — enforce the per-identity contact limit.
        let count = self.storage.contacts().count_contacts()?;
        let limit = self.storage.contacts().get_contact_limit()?;
        if count >= limit {
            return Err(VauchiError::InvalidState(format!(
                "contact limit reached ({limit})"
            )));
        }
        let now = self.clock.unix_seconds();
        let id = uuid::Uuid::new_v4().to_string();
        let contact = Contact::from_import(id, card, ImportSource::LinkExchange, Some(uid), now);
        self.storage.contacts().save_contact(&contact)?;
        Ok(contact.id().to_string())
    }

    /// Complete a link-mode exchange from a received card payload (ADR-050
    /// Phase 2, T5b). For a **v2** symmetric bootstrap this establishes a
    /// *live, updatable* `Exchanged` contact: it derives the symmetric link
    /// shared key from `our_x3dh` (the retained per-exchange keypair whose
    /// public half we deposited) and the peer's signed X3DH key, saves the
    /// contact with `ExchangeTransport::Link` + the peer's relay routing,
    /// and initializes the Double Ratchet with a deterministic role. A
    /// **v1** payload carries no exchange key, so it falls back to
    /// [`Self::import_received_link_card`] (a frozen import, no channel).
    ///
    /// **Ratchet role:** the smaller identity key is the initiator — the
    /// same rule as in-person exchange
    /// (`ExchangeSession::build_exchange_ratchet`), so link contacts share
    /// one role convention. The initiator keys off the peer's X3DH public;
    /// the responder off our retained keypair (it learns the initiator's DH
    /// from the first message). Both sides hold both keys, so the role can
    /// be derived from identity ordering alone — no flow-role plumbing.
    ///
    /// Idempotent: re-receiving the same peer (contact id = `hex(identity
    /// pubkey)`) returns the existing contact and keeps its ratchet rather
    /// than re-keying an in-flight channel. Enforces the per-identity
    /// contact limit. Rejects the degenerate self-exchange (our own
    /// bootstrap). Returns the contact id.
    pub fn complete_link_exchange(
        &self,
        card_bytes: &[u8],
        our_x3dh: &crate::exchange::X3DHKeyPair,
    ) -> VauchiResult<String> {
        use crate::crypto::DoubleRatchetState;
        use crate::exchange::ExchangeError;
        use crate::exchange::link_mode::{
            LinkCardPayload, derive_link_shared_key, parse_card_payload_versioned,
        };

        let payload = parse_card_payload_versioned(card_bytes)
            .map_err(|e| VauchiError::Exchange(ExchangeError::InvalidState(e.to_string())))?;

        let (identity_pubkey, x3dh_pubkey, relay_url, card) = match payload {
            // Legacy v1 has no exchange key — frozen import, no channel.
            LinkCardPayload::V1 { card, .. } => return self.import_received_link_card(card),
            LinkCardPayload::V2 {
                identity_pubkey,
                x3dh_pubkey,
                relay_url,
                card,
                ..
            } => (identity_pubkey, x3dh_pubkey, relay_url, card),
        };

        let our_identity = *self
            .identity()
            .ok_or_else(|| {
                VauchiError::InvalidState("no identity — cannot complete a link exchange".into())
            })?
            .signing_public_key();

        // Reject our own bootstrap (degenerate self-exchange) before any write.
        if identity_pubkey == our_identity {
            return Err(VauchiError::InvalidState(
                "cannot complete a link exchange with our own identity".into(),
            ));
        }

        let contact_id = hex::encode(identity_pubkey);

        // Idempotent: keep the existing contact and its ratchet — re-keying
        // would desync an already-established channel.
        if self.storage.contacts().load_contact(&contact_id)?.is_some() {
            return Ok(contact_id);
        }

        // Enforce the per-identity contact limit (C3) — mirrors the import path.
        let count = self.storage.contacts().count_contacts()?;
        let limit = self.storage.contacts().get_contact_limit()?;
        if count >= limit {
            return Err(VauchiError::InvalidState(format!(
                "contact limit reached ({limit})"
            )));
        }

        // Symmetric link shared key (commutative DH — both sides derive the
        // same key, ADR-050), authenticated by the peer's identity signature
        // over the bootstrap (verified during parsing).
        let shared_key = derive_link_shared_key(our_x3dh, &x3dh_pubkey)
            .map_err(|e| VauchiError::Exchange(ExchangeError::KeyAgreementFailed(e.to_string())))?;

        let now = self.clock.unix_seconds();
        let contact = Contact::from_link_exchange(
            identity_pubkey,
            card,
            shared_key.clone(),
            Some(relay_url),
            now,
        );
        self.add_contact(contact)?;

        // Deterministic Double Ratchet role (decision (b)): smaller identity
        // key = initiator. The initiator keys off the peer's X3DH public; the
        // responder off our retained keypair. Persisted with the chosen role
        // via `save_exchange_ratchet`, which never guesses it.
        let is_initiator = our_identity < identity_pubkey;
        let ratchet = if is_initiator {
            DoubleRatchetState::initialize_initiator(&shared_key, x3dh_pubkey)
                .map_err(|e| VauchiError::Crypto(e.to_string()))?
        } else {
            let our_dh = crate::exchange::X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes());
            DoubleRatchetState::initialize_responder(&shared_key, our_dh)
        };
        self.save_exchange_ratchet(&contact_id, &ratchet, is_initiator)?;

        Ok(contact_id)
    }
}

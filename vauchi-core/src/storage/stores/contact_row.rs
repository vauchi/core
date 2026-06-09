// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact domain persistence view — row mappers (impl ContactStore).

use crate::crypto::SymmetricKey;

use super::super::StorageError;
use super::ContactStore;
use crate::contact::Contact;
use crate::contact::ImportSource;
use crate::contact_card::ContactCard;
use crate::crypto::cek::ContentEncryptionKey;
use crate::exchange::TrustMetrics;
use crate::exchange::reciprocity::{ConfirmationChannel, Reciprocity};
use crate::types::ExchangeTransport;

/// Internal struct for database row data.
#[allow(dead_code)] // Fields are used via destructuring in row_to_contact
pub(super) struct ContactRow {
    pub id: String,
    pub public_key: Vec<u8>,
    pub display_name: String,
    pub card_encrypted: Vec<u8>,
    pub shared_key_encrypted: Vec<u8>,
    pub visibility_rules_json: Option<String>,
    pub visibility_rules_encrypted: Option<Vec<u8>>,
    pub exchange_timestamp: i64,
    pub fingerprint_verified: i32,
    pub blocked: i32,
    pub hidden: i32,
    pub favorite: i32,
    pub recovery_trusted: i32,
    pub proposal_trusted: i32,
    pub cek_encrypted: Option<Vec<u8>>,
    pub exchange_transport: String,
    pub has_recovered: i32,
    pub card_updated_at: Option<i64>,
    pub relay_url: Option<String>,
    pub relay_noise_pubkey: Option<Vec<u8>>,
    pub trust_metrics: Option<String>,
    pub contact_kind: String,
    pub import_source: Option<String>,
    pub imported_at: Option<i64>,
    pub original_uid: Option<String>,
    pub deleted_at: Option<i64>,
    pub archived: i32,
    pub archived_at: Option<i64>,
    pub reciprocity: Option<String>,
    pub confirmation_channel: Option<String>,
}

impl ContactStore<'_> {
    /// Converts a database row to a Contact.
    ///
    /// CEK-aware: if `cek_encrypted` is present, decrypts the CEK with the
    /// storage key, then decrypts the card with the CEK. Otherwise, decrypts
    /// the card with the storage key (legacy path).
    pub(super) fn row_to_contact(&self, row: ContactRow) -> Result<Contact, StorageError> {
        let (card, cek) = if let Some(ref cek_encrypted) = row.cek_encrypted {
            let cek_bytes = crate::crypto::decrypt(self.key, cek_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let cek_array: [u8; 32] = cek_bytes
                .try_into()
                .map_err(|_| StorageError::Encryption("Invalid CEK length".into()))?;
            let cek = ContentEncryptionKey::from_bytes(cek_array);

            let card_json = cek
                .decrypt(&row.card_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            (card, Some(cek))
        } else {
            let card_json = crate::crypto::decrypt(self.key, &row.card_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            (card, None)
        };

        if row.contact_kind == "imported" {
            return self.row_to_imported_contact(row, card, cek);
        }

        // === Exchanged contact path (default, backward-compatible) ===
        self.row_to_exchanged_contact(row, card, cek)
    }
    /// Reconstructs an imported contact from a database row.
    fn row_to_imported_contact(
        &self,
        row: ContactRow,
        card: ContactCard,
        cek: Option<ContentEncryptionKey>,
    ) -> Result<Contact, StorageError> {
        let source: ImportSource = row
            .import_source
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| StorageError::Serialization(e.to_string()))?
            .unwrap_or(ImportSource::Manual);

        let imported_at = row.imported_at.unwrap_or(0) as u64;

        let mut contact =
            Contact::from_import_stored(row.id, card, source, imported_at, row.original_uid);

        if row.blocked != 0 {
            contact.set_blocked(true);
        }
        if row.hidden != 0 {
            contact.set_hidden(true);
        }
        if row.favorite != 0 {
            contact.set_favorite(true);
        }
        contact.set_card_updated_at(row.card_updated_at.map(|t| t as u64));

        if let Some(ts) = row.deleted_at {
            contact.soft_delete(ts as u64);
        }
        if row.archived != 0 {
            if let Some(ts) = row.archived_at {
                contact.archive(ts as u64);
            } else {
                contact.archive(0);
            }
        }

        if let Some(cek) = cek {
            contact.set_cek(cek);
        }

        Ok(contact)
    }
    /// Reconstructs an exchanged contact from a database row.
    fn row_to_exchanged_contact(
        &self,
        row: ContactRow,
        card: ContactCard,
        cek: Option<ContentEncryptionKey>,
    ) -> Result<Contact, StorageError> {
        let shared_key_bytes = crate::crypto::decrypt(self.key, &row.shared_key_encrypted)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let shared_key_array: [u8; 32] = shared_key_bytes
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid key length".into()))?;
        let shared_key = SymmetricKey::from_bytes_unchecked(shared_key_array);

        // Parse public key — HR-2: empty blob from imported contact fails safely here
        let public_key: [u8; 32] = row
            .public_key
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid public key length".into()))?;

        // Parse visibility rules — prefer encrypted column, fall back to legacy plaintext
        let visibility_rules = if let Some(encrypted) = row.visibility_rules_encrypted {
            let json_bytes = crate::crypto::decrypt(self.key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let json = String::from_utf8(json_bytes)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else if let Some(json) = row.visibility_rules_json {
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else {
            crate::contact::VisibilityRules::new()
        };

        let mut contact = Contact::from_sync_data_full(
            public_key,
            card,
            shared_key,
            row.exchange_timestamp as u64,
            row.fingerprint_verified != 0,
            visibility_rules,
            row.hidden != 0,
            row.blocked != 0,
            row.recovery_trusted != 0,
        );

        if row.favorite != 0 {
            contact.set_favorite(true);
        }

        // Restore proposal_trusted flag from storage. best-effort: if
        // the contact's trust level has drifted (e.g. now blocked since
        // save), set() returns Err and the flag falls back to default
        if row.proposal_trusted != 0 {
            #[allow(clippy::let_underscore_must_use)]
            let _ = contact.set_proposal_trusted(true);
        }

        if let Some(cek) = cek {
            contact.set_cek(cek);
        }

        // Restore trust metric fields from storage.
        //
        // Site 8 of `2026-05-21-silent-failures-in-security-paths`:
        // `exchange_transport` is the ADR-034 trust-derivation input. A
        // corrupt or downgraded column used to fall back to `Default`
        // (`Qr`) via `unwrap_or_default()`, silently producing a wrong
        // trust badge. Propagate the deserialization error as
        // `StorageError::Serialization` so the contact load fails loudly
        // — that's the only honest signal for an ADR-034 input we
        // couldn't parse.
        let transport: ExchangeTransport =
            serde_json::from_value(serde_json::Value::String(row.exchange_transport.clone()))
                .map_err(|e| {
                    StorageError::Serialization(format!(
                        "row_to_contact: exchange_transport column unparsable ({e}); ADR-034 \
                 trust-derivation input must round-trip — raw value: {raw:?}",
                        raw = row.exchange_transport
                    ))
                })?;
        contact.set_exchange_transport(transport);
        contact.set_has_recovered(row.has_recovered != 0);
        contact.set_card_updated_at(row.card_updated_at.map(|t| t as u64));

        contact.set_relay_url(row.relay_url);
        if let Some(pubkey_bytes) = row.relay_noise_pubkey {
            let pubkey: [u8; 32] = pubkey_bytes.try_into().map_err(|_| {
                StorageError::Encryption("Invalid relay Noise pubkey length".into())
            })?;
            contact.set_relay_noise_pubkey(Some(pubkey));
        }

        // Restore trust metrics from storage (JSON column, NULL for legacy contacts).
        //
        // Site 8 peer: enrichment field — a corrupt row should still let
        // the contact load (so the user sees their name + can re-exchange
        // to repair) but the failure must not be silent. Surface via
        // tracing::warn! and fall back to None.
        let trust_metrics: Option<TrustMetrics> =
            row.trust_metrics
                .and_then(|s| match serde_json::from_str(&s) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        tracing::warn!(
                            target: "vauchi.storage.contact_row",
                            contact_id = %row.id,
                            error = %e,
                            "trust_metrics column unparsable; falling back to None"
                        );
                        None
                    }
                });
        contact.set_trust_metrics(trust_metrics);

        // Restore reciprocity from storage (TEXT column, NULL for legacy contacts).
        // Site 8 peer: same shape — log + fallback.
        if let Some(r) = row.reciprocity.and_then(|s| {
            match serde_json::from_value::<Reciprocity>(serde_json::Value::String(s)) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!(
                        target: "vauchi.storage.contact_row",
                        contact_id = %row.id,
                        error = %e,
                        "reciprocity column unparsable; leaving contact reciprocity un-set"
                    );
                    None
                }
            }
        }) {
            contact.set_reciprocity(r);
        }
        // Site 8 peer: same shape — log + fallback.
        if let Some(c) = row.confirmation_channel.and_then(|s| {
            match serde_json::from_value::<ConfirmationChannel>(serde_json::Value::String(s)) {
                Ok(c) => Some(c),
                Err(e) => {
                    tracing::warn!(
                        target: "vauchi.storage.contact_row",
                        contact_id = %row.id,
                        error = %e,
                        "confirmation_channel column unparsable; leaving un-set"
                    );
                    None
                }
            }
        }) {
            contact.set_confirmation_channel(c);
        }

        if let Some(ts) = row.deleted_at {
            contact.soft_delete(ts as u64);
        }
        if row.archived != 0 {
            if let Some(ts) = row.archived_at {
                contact.archive(ts as u64);
            } else {
                contact.archive(0);
            }
        }

        Ok(contact)
    }
}

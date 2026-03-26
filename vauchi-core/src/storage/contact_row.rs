// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Database row type and row-to-domain conversion for contacts.

use super::{Storage, StorageError};
use crate::contact::Contact;
use crate::contact::ImportSource;
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::crypto::cek::ContentEncryptionKey;
use crate::exchange::TrustMetrics;
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
}

impl Storage {
    /// Converts a database row to a Contact.
    ///
    /// CEK-aware: if `cek_encrypted` is present, decrypts the CEK with the
    /// storage key, then decrypts the card with the CEK. Otherwise, decrypts
    /// the card with the storage key (legacy path).
    pub(super) fn row_to_contact(&self, row: ContactRow) -> Result<Contact, StorageError> {
        // Decrypt card — CEK path or legacy path
        let (card, cek) = if let Some(ref cek_encrypted) = row.cek_encrypted {
            // CEK path: decrypt CEK with storage key, then card with CEK
            let cek_bytes = crate::crypto::decrypt(&self.encryption_key, cek_encrypted)
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
            // Legacy path: decrypt card with storage key
            let card_json = crate::crypto::decrypt(&self.encryption_key, &row.card_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            (card, None)
        };

        // Branch on contact_kind: imported contacts skip all crypto column parsing
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

        // Restore local-only flags
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
        // Decrypt shared key
        let shared_key_bytes =
            crate::crypto::decrypt(&self.encryption_key, &row.shared_key_encrypted)
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
            let json_bytes = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let json = String::from_utf8(json_bytes)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else if let Some(json) = row.visibility_rules_json {
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else {
            crate::contact::VisibilityRules::new()
        };

        // Create contact with all persisted fields
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

        // Restore favorite flag from storage
        if row.favorite != 0 {
            contact.set_favorite(true);
        }

        // Restore proposal_trusted flag from storage
        if row.proposal_trusted != 0 {
            let _ = contact.set_proposal_trusted(true);
        }

        // Attach CEK if this contact is CEK-protected
        if let Some(cek) = cek {
            contact.set_cek(cek);
        }

        // Restore trust metric fields from storage
        let transport: ExchangeTransport =
            serde_json::from_value(serde_json::Value::String(row.exchange_transport))
                .unwrap_or_default();
        contact.set_exchange_transport(transport);
        contact.set_has_recovered(row.has_recovered != 0);
        contact.set_card_updated_at(row.card_updated_at.map(|t| t as u64));

        // Restore relay fields from storage
        contact.set_relay_url(row.relay_url);
        if let Some(pubkey_bytes) = row.relay_noise_pubkey {
            let pubkey: [u8; 32] = pubkey_bytes.try_into().map_err(|_| {
                StorageError::Encryption("Invalid relay Noise pubkey length".into())
            })?;
            contact.set_relay_noise_pubkey(Some(pubkey));
        }

        // Restore trust metrics from storage (JSON column, NULL for legacy contacts)
        let trust_metrics: Option<TrustMetrics> = row
            .trust_metrics
            .and_then(|s| serde_json::from_str(&s).ok());
        contact.set_trust_metrics(trust_metrics);

        Ok(contact)
    }
}

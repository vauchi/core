// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preserve contact state independently of the public legacy backup envelope.

use serde::{Deserialize, Serialize};

use crate::contact::Contact;
use crate::exchange::{ConfirmationChannel, Reciprocity, TrustMetrics};
use crate::{ExchangeTransport, ProximityConfidence};

use super::super::BackupError;
use super::super::contact_backup::ContactBackupEntry;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotContact {
    entry: ContactBackupEntry,
    hidden: bool,
    blocked: bool,
    favorite: bool,
    archived_at: Option<u64>,
    ignored_at: Option<u64>,
    card_updated_at: Option<u64>,
    relationship: Option<RelationshipState>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationshipState {
    exchange_transport: ExchangeTransport,
    proposal_trusted: bool,
    proximity_confidence: ProximityConfidence,
    has_recovered: bool,
    relay_url: Option<String>,
    trust_metrics: Option<TrustMetrics>,
    reciprocity: Option<Reciprocity>,
    confirmation_channel: Option<ConfirmationChannel>,
}

impl SnapshotContact {
    pub fn capture(contact: &Contact) -> Result<Self, BackupError> {
        let relationship = contact
            .kind()
            .exchanged_data()
            .map(|data| RelationshipState {
                exchange_transport: data.exchange_transport,
                proposal_trusted: data.proposal_trusted,
                proximity_confidence: data.proximity_confidence,
                has_recovered: data.has_recovered,
                relay_url: data.relay_url.clone(),
                trust_metrics: data.trust_metrics.clone(),
                reciprocity: data.reciprocity,
                confirmation_channel: data.confirmation_channel,
            });
        Ok(Self {
            entry: ContactBackupEntry::from_contact(contact)?,
            hidden: contact.is_hidden(),
            blocked: contact.is_blocked(),
            favorite: contact.is_favorite(),
            archived_at: contact.archived_at(),
            ignored_at: contact.ignored_at(),
            card_updated_at: contact.card_updated_at(),
            relationship,
        })
    }

    pub fn from_legacy(entry: ContactBackupEntry) -> Result<Self, BackupError> {
        Self::capture(&entry.to_contact()?)
    }

    pub fn restore(&self) -> Result<Contact, BackupError> {
        let mut contact = self.entry.to_contact()?;
        contact.set_hidden(self.hidden);
        contact.set_blocked(self.blocked);
        contact.set_favorite(self.favorite);
        contact.set_card_updated_at(self.card_updated_at);
        if let Some(at) = self.archived_at {
            contact.archive(at);
        }
        if let Some(at) = self.ignored_at {
            contact.ignore(at);
        }
        if let Some(state) = &self.relationship {
            contact.set_exchange_transport(state.exchange_transport);
            contact
                .set_proposal_trusted(state.proposal_trusted)
                .map_err(|_| BackupError::Deserialization("Invalid contact relationship".into()))?;
            contact.set_proximity_confidence(state.proximity_confidence);
            contact.set_has_recovered(state.has_recovered);
            contact.set_relay_url(state.relay_url.clone());
            contact.set_trust_metrics(state.trust_metrics.clone());
            if let Some(reciprocity) = state.reciprocity {
                contact.set_reciprocity(reciprocity);
            }
            if let Some(channel) = state.confirmation_channel {
                contact.set_confirmation_channel(channel);
            }
        }
        Ok(contact)
    }
}

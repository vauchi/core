// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local ordering survives sibling-device delivery and removed card fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{CardDelta, CardUpdateError, FieldChange, Storage, StorageError};
use crate::contact_card::ContactCard;

/// Never evict tombstones: eviction would let a held old addition restore a
/// removed field. A full contact fails closed until its relationship is reset.
const MAX_ORDERING_KEYS: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Stamp {
    timestamp: u64,
    origin_device: [u8; 32],
    delta_version: u32,
}

pub(super) struct OrderedCard {
    pub card: ContactCard,
    pub state: Vec<u8>,
    pub removed_fields: Vec<String>,
}

/// Called after signature verification, inside the receive write transaction.
/// The signed field time orders re-propagated additions; the signed delta time
/// orders removals and names. Device versions break same-origin, same-time ties.
/// Equal legacy stamps have no causal distinction and retain apply semantics.
pub(super) fn apply(
    storage: &Storage,
    contact_id: &str,
    mut card: ContactCard,
    delta: &CardDelta,
    origin_device: &[u8; 32],
) -> Result<OrderedCard, CardUpdateError> {
    let mut stamps: BTreeMap<String, Stamp> =
        match storage.contacts().load_card_ordering(contact_id)? {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| StorageError::Serialization(e.to_string()))?,
            None => BTreeMap::new(),
        };
    // Upgrade can preserve live field times. Previously removed fields have no
    // recoverable timestamp; only removals accepted after upgrade are protected.
    card.deduplicate_fields();
    for field in card.fields() {
        stamps
            .entry(format!("field:{}", field.id()))
            .or_insert(Stamp {
                timestamp: field.updated_at(),
                origin_device: [0; 32],
                delta_version: 0,
            });
    }
    if stamps.len() > MAX_ORDERING_KEYS {
        return Err(CardUpdateError::OrderingCapacityExceeded);
    }

    let mut accepted = delta.clone();
    accepted.changes.clear();
    let mut removed_fields = Vec::new();
    for change in &delta.changes {
        let (key, timestamp) = match change {
            FieldChange::Added { field } => (format!("field:{}", field.id()), field.updated_at()),
            FieldChange::Removed { field_id } => (format!("field:{field_id}"), delta.timestamp),
            FieldChange::DisplayNameChanged { .. } => ("name".to_string(), delta.timestamp),
        };
        let incoming = Stamp {
            timestamp,
            origin_device: *origin_device,
            delta_version: delta.version,
        };
        if stamps.get(&key).is_some_and(|last| incoming < *last) {
            continue;
        }
        if !stamps.contains_key(&key) && stamps.len() == MAX_ORDERING_KEYS {
            return Err(CardUpdateError::OrderingCapacityExceeded);
        }
        stamps.insert(key, incoming);
        if let FieldChange::Removed { field_id } = change {
            removed_fields.push(field_id.clone());
        }
        accepted.changes.push(change.clone());
    }
    accepted
        .apply(&mut card)
        .map_err(|_| CardUpdateError::DeltaApplicationFailed)?;
    let state =
        serde_json::to_vec(&stamps).map_err(|e| StorageError::Serialization(e.to_string()))?;
    Ok(OrderedCard {
        card,
        state,
        removed_fields,
    })
}

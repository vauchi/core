// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Assembles the paper heirloom book (private#363).

use std::collections::HashMap;

use super::Vauchi;
use crate::api::error::VauchiResult;
use crate::api::heirloom::{HeirloomBook, HeirloomContact, HeirloomField};

impl Vauchi {
    /// The contact book as plain, human-facing content, sorted by name and
    /// then exchange date so the same data always yields the same book.
    ///
    /// Reads through the duress-aware APIs: under the duress PIN the book
    /// holds the decoy contacts only (ADR-032).
    pub fn heirloom_book(&self) -> VauchiResult<HeirloomBook> {
        let place_names: HashMap<String, String> = self
            .list_places()?
            .into_iter()
            .map(|place| (place.id, place.name))
            .collect();
        // One bulk read: a per-contact lookup errors on decoys, which have no
        // row in the duress store's contact table.
        let place_ids: HashMap<String, String> = self
            .vocabulary_store()?
            .list_exchange_locations()?
            .into_iter()
            .filter_map(|(contact_id, loc)| loc.place_id.map(|id| (contact_id, id)))
            .collect();
        let mut contacts = Vec::new();
        for contact in self.list_contacts()? {
            let place = place_ids
                .get(contact.id())
                .and_then(|id| place_names.get(id).cloned());
            contacts.push(HeirloomContact {
                name: contact.display_name().to_string(),
                fields: contact
                    .card()
                    .fields()
                    .iter()
                    .map(|f| HeirloomField {
                        label: f.label().to_string(),
                        value: f.value().to_string(),
                    })
                    .collect(),
                exchanged_at: contact.exchange_timestamp(),
                place,
                avatar_webp: contact.card().avatar().map(<[u8]>::to_vec),
            });
        }
        contacts.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.exchanged_at.cmp(&b.exchanged_at))
        });
        let owner_name = self
            .own_card()?
            .map(|card| card.display_name().to_string())
            .unwrap_or_default();
        Ok(HeirloomBook {
            owner_name,
            contacts,
        })
    }
}

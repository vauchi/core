// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Faceted contact search (ADR-051 contact annotations).
//!
//! Plain [`Vauchi::search_contacts`] matches display name only — the default.
//! [`Vauchi::search_contacts_faceted`] opts into matching the owner's private
//! annotations too: tags, the comment (`personal_notes`), and the named
//! exchange place, plus an exchange-time range filter. Core-owned (ADR-021);
//! frontends pass flags, never re-implement matching.

use super::super::error::VauchiResult;
use super::Vauchi;
use crate::contact::Contact;

/// Which annotation facets a faceted search should match, beyond display name.
///
/// All-false is equivalent to plain name search. `time_range`, when set, is an
/// **AND** filter on each contact's `acquired_at` (the text facets are OR-ed
/// with the name match).
#[derive(Debug, Clone, Default)]
pub struct SearchFacets {
    /// Match the query against the contact's tag names.
    pub tags: bool,
    /// Match the query against the contact's comment (`personal_notes`).
    pub comment: bool,
    /// Match the query against the contact's named exchange place.
    pub place: bool,
    /// Restrict to contacts acquired within this inclusive `(from, to)` epoch
    /// range (seconds). Applied as an AND filter regardless of the text query.
    pub time_range: Option<(u64, u64)>,
}

impl Vauchi {
    /// Faceted contact search. A non-hidden contact is included when it passes
    /// the optional `time_range` filter **and** either the query is empty or it
    /// text-matches (case-insensitive substring) the display name or any
    /// enabled facet (tags / comment / place name).
    ///
    /// With `SearchFacets::default()` (no facets, no range) this matches
    /// [`Vauchi::search_contacts`] — name only.
    pub fn search_contacts_faceted(
        &self,
        query: &str,
        facets: &SearchFacets,
    ) -> VauchiResult<Vec<Contact>> {
        let q = query.trim().to_lowercase();

        // Hoist the tag vocabulary once (avoids re-loading per contact).
        let tags = if facets.tags {
            self.storage.list_tags()?
        } else {
            Vec::new()
        };

        let mut out = Vec::new();
        for contact in self.storage.list_contacts()? {
            if contact.is_hidden() {
                continue;
            }

            // Time range is an AND filter on acquisition time.
            if let Some((from, to)) = facets.time_range {
                let at = contact.acquired_at();
                if at < from || at > to {
                    continue;
                }
            }

            if self.contact_text_matches(&contact, &q, facets, &tags)? {
                out.push(contact);
            }
        }
        Ok(out)
    }

    /// True if the query is empty or matches the name or any enabled facet.
    fn contact_text_matches(
        &self,
        contact: &Contact,
        q: &str,
        facets: &SearchFacets,
        tags: &[crate::contact::Tag],
    ) -> VauchiResult<bool> {
        if q.is_empty() {
            return Ok(true);
        }
        if contact.display_name().to_lowercase().contains(q) {
            return Ok(true);
        }
        if facets.tags
            && tags
                .iter()
                .any(|t| t.contains(contact.id()) && t.name.to_lowercase().contains(q))
        {
            return Ok(true);
        }
        // Comment lives in personal_notes, which only exchanged contacts have.
        if facets.comment
            && contact.is_exchanged()
            && self
                .read_personal_note(contact.id())?
                .is_some_and(|n| n.to_lowercase().contains(q))
        {
            return Ok(true);
        }
        if facets.place
            && let Some(loc) = self.storage.load_exchange_location(contact.id())?
            && let Some(place_id) = &loc.place_id
            && let Some(place) = self.storage.get_place(place_id)?
            && place.name.to_lowercase().contains(q)
        {
            return Ok(true);
        }
        Ok(false)
    }
}

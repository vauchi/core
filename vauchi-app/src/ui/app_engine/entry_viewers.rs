// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::Vauchi;

use crate::ui::my_info_entry_detail::{EntryContactInfo, EntryViewerVia};

/// The contacts an own-card entry is sent to, each with the reason.
///
/// Membership comes from `get_effective_field_visibility`, the rule that
/// filters outbound card updates, so this list cannot disagree with what
/// contacts receive (#425: it used to count group grants only, so an entry
/// shared with all contacts read as seen by nobody).
pub(super) fn entry_viewers(vauchi: &Vauchi, field_id: &str) -> Vec<EntryContactInfo> {
    let contacts = vauchi.list_contacts().unwrap_or_default();
    let groups = vauchi.list_groups().unwrap_or_default();
    let shown_to_all = vauchi
        .own_card()
        .ok()
        .flatten()
        .is_some_and(|card| card.is_field_shown(field_id));

    contacts
        .iter()
        .filter(|contact| {
            vauchi
                .get_effective_field_visibility(contact.id(), field_id)
                .unwrap_or(false)
        })
        .map(|contact| {
            let granting_group = groups.iter().find(|g| {
                g.is_field_visible(field_id) && g.contacts().iter().any(|c| c == contact.id())
            });
            let via = match granting_group {
                Some(group) => EntryViewerVia::Group(group.name().to_string()),
                None if shown_to_all => EntryViewerVia::Everyone,
                None => EntryViewerVia::Individual,
            };
            EntryContactInfo {
                contact_id: contact.id().to_string(),
                name: contact.display_name().to_string(),
                via,
            }
        })
        .collect()
}

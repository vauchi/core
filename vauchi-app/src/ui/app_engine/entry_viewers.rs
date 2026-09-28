// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::Vauchi;

use crate::ui::my_info_entry_detail::EntryContactInfo;

/// The contacts listed on an own-card entry's detail screen as able to
/// see it, each with the group that grants it.
pub(super) fn entry_viewers(vauchi: &Vauchi, field_id: &str) -> Vec<EntryContactInfo> {
    let all_groups = vauchi.list_groups().unwrap_or_default();
    let mut visible_contacts = Vec::new();
    let mut seen_contacts = std::collections::HashSet::new();
    for g in &all_groups {
        if g.is_field_visible(field_id) {
            for cid in g.contacts() {
                if seen_contacts.insert(cid.to_string()) {
                    let name = vauchi
                        .get_contact(cid)
                        .ok()
                        .flatten()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_else(|| "Unknown".into());
                    visible_contacts.push(EntryContactInfo {
                        contact_id: cid.to_string(),
                        name,
                        via_group: g.name().to_string(),
                    });
                }
            }
        }
    }
    visible_contacts
}

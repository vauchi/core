// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact card, contact CRUD, hidden contacts, and social network operations.

use super::types::MobileContact;

/// Enriches a single contact with display context (nickname, resolved name, custom avatar flag).
///
/// Use this for single-contact operations (`get_contact`). For list operations use
/// `enrich_contacts_batch` to avoid N+1 queries.
///
/// Promoted from a private associated function on `VauchiPlatform` to
/// a `pub(crate)` free function so `PlatformAppEngine` dispatch arms
/// (B7 batch 10/11) can reuse the same enrichment helper.
pub(crate) fn enrich_contact(
    storage: &vauchi_core::Storage,
    contact: &vauchi_core::Contact,
) -> MobileContact {
    let cid = contact.id();
    let nickname = storage.contacts().load_contact_nickname(cid).ok().flatten();
    let has_custom_avatar = storage
        .contacts()
        .has_contact_custom_avatar(cid)
        .unwrap_or(false);
    let shared_names = storage
        .contacts()
        .list_shared_names(cid)
        .unwrap_or_default();
    let (name_pref, _avatar_pref) = storage.contacts().load_display_preferences(cid).unwrap_or((
        vauchi_core::DisplayNamePreference::Primary,
        vauchi_core::AvatarPreference::Primary,
    ));
    let resolved = vauchi_core::contact::display::resolve_display_name(
        contact.display_name(),
        &name_pref,
        &shared_names,
        nickname.as_deref(),
    );
    MobileContact::with_display_context(contact, nickname, resolved, has_custom_avatar)
}

/// Batch-enriches a slice of contacts using a single round of queries per data type.
///
/// Issues four queries total (shared names, nicknames, preferences, avatar flags)
/// regardless of the number of contacts, eliminating the N+1 pattern in list methods.
///
/// Promoted from a private associated function on `VauchiPlatform` to
/// a `pub(crate)` free function so `PlatformAppEngine` dispatch arms
/// (B7 batch 10/11) can reuse the same enrichment helper.
pub(crate) fn enrich_contacts_batch(
    storage: &vauchi_core::Storage,
    contacts: &[vauchi_core::Contact],
) -> Vec<MobileContact> {
    if contacts.is_empty() {
        return Vec::new();
    }
    let ids: Vec<&str> = contacts.iter().map(|c| c.id()).collect();

    let shared_names_map = storage
        .contacts()
        .batch_shared_names(&ids)
        .unwrap_or_default();
    let nicknames_map = storage.contacts().batch_nicknames(&ids).unwrap_or_default();
    let prefs_map = storage
        .contacts()
        .batch_display_preferences(&ids)
        .unwrap_or_default();
    let has_avatar_set = storage
        .contacts()
        .batch_has_custom_avatar(&ids)
        .unwrap_or_default();

    contacts
        .iter()
        .map(|contact| {
            let cid = contact.id();
            let nickname = nicknames_map.get(cid).cloned();
            let has_custom_avatar = has_avatar_set.contains(cid);
            let shared_names = shared_names_map.get(cid).cloned().unwrap_or_default();
            let (name_pref, _avatar_pref) = prefs_map.get(cid).cloned().unwrap_or((
                vauchi_core::DisplayNamePreference::Primary,
                vauchi_core::AvatarPreference::Primary,
            ));
            let resolved = vauchi_core::contact::display::resolve_display_name(
                contact.display_name(),
                &name_pref,
                &shared_names,
                nickname.as_deref(),
            );
            MobileContact::with_display_context(contact, nickname, resolved, has_custom_avatar)
        })
        .collect()
}

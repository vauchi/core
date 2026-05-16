// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility helpers shared by `DomainCommand::GetLabel`.
//!
//! The `impl VauchiPlatform { … }` UniFFI surface retired in slice 32b
//! of the Phase 2 vauchi-platform collapse (problem record
//! `2026-05-16-slice-32b-mobile-visibility-retirement`). All 15
//! visibility methods migrated to `DomainCommand` handlers in
//! `platform_app_engine.rs`; frontends route via
//! `dispatchDomainCommand`. `resolve_label_contacts` stayed because
//! the surviving `DomainCommand::GetLabel` handler still calls it.

use super::types::{MobileLabelContactBadge, MobileLabelContactRow, MobileLabelContactStatus};

/// Resolve raw contact IDs against storage into rendered rows.
///
/// Active contacts produce `MobileLabelContactRow` entries with the same
/// display-name pipeline `enrich_contact()` uses for `list_contacts` (so a
/// contact with a nickname renders the same in both surfaces). Missing or
/// errored IDs are dropped from the rows and counted in the second tuple
/// member; this is the conservative default per the planning record's
/// missing-contact policy decision (`omit + stale_reference_count`).
///
/// Order is preserved: the i-th row corresponds to the next active id from
/// `contact_ids` left-to-right. The invariant
/// `rows.len() + stale_count as usize == contact_ids.len()` is verified in
/// `mobile_visibility_resolve_tests`.
pub(crate) fn resolve_label_contacts(
    storage: &vauchi_core::Storage,
    contact_ids: &[String],
) -> (Vec<MobileLabelContactRow>, u32) {
    let mut rows = Vec::with_capacity(contact_ids.len());
    let mut stale: u32 = 0;

    for id in contact_ids {
        match storage.load_contact(id) {
            Ok(Some(contact)) => {
                let nickname = storage.load_contact_nickname(id).ok().flatten();
                let shared_names = storage.list_shared_names(id).unwrap_or_default();
                let (name_pref, _) = storage.load_display_preferences(id).unwrap_or((
                    vauchi_core::DisplayNamePreference::Primary,
                    vauchi_core::AvatarPreference::Primary,
                ));
                let display_name = vauchi_core::contact::display::resolve_display_name(
                    contact.display_name(),
                    &name_pref,
                    &shared_names,
                    nickname.as_deref(),
                );
                let mut badges = Vec::new();
                if contact.is_fingerprint_verified() {
                    badges.push(MobileLabelContactBadge::Verified);
                }
                rows.push(MobileLabelContactRow {
                    id: id.clone(),
                    display_name,
                    trust_level: contact.trust_level().into(),
                    status: MobileLabelContactStatus::Active,
                    badges,
                });
            }
            // Missing or error → omit from rows and bump stale_reference_count.
            // Per the planning record (G2 missing-contact policy default):
            // never expose unresolved contact IDs to the UI; surface the
            // count instead so the frontend can render a footer hint.
            _ => {
                stale = stale.saturating_add(1);
            }
        }
    }

    (rows, stale)
}

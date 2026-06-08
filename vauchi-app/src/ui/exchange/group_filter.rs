// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange-time field-visibility resolver.
//!
//! Computes the allow-list of card field ids that the selected exchange
//! group(s) may receive. This is the single source of truth feeding the
//! field-bearing exchange payloads (BLE `BleCardPayload`, Cable
//! `DirectTransport`) via [`ContactCard::filtered_to`], plus the pre-handoff
//! preview and success summary — so they cannot diverge.
//!
//! Model (see `_private/docs/problems/2026-06-08-exchange-card-not-group-filtered/
//! investigation.md`): the exchange audience is a **group's `visible_fields`
//! allow-list** (Layer B, group-keyed — the only layer evaluable before the
//! peer has a contact id), intersected with the Layer-A `Nobody` floor:
//!
//! ```text
//! allow = (⋃_{g ∈ selected} g.visible_fields) \ {f : default(f) == Nobody}
//! ```
//!
//! `Contacts(id)` / per-contact overrides are post-exchange concerns and take
//! effect on the next propagation once the contact id exists.

use std::collections::HashSet;

use vauchi_core::{FieldVisibility, Group, VisibilityRules};

/// Resolves the field-id allow-list for an exchange audience.
///
/// - `None` → **no group filter** (share all fields). Returned when no groups
///   are selected — there is no audience to restrict to (this is the legacy
///   no-groups case and is correct).
/// - `Some(set)` → share exactly `set`, the union of the selected groups'
///   `visible_fields` minus any field whose card default visibility is
///   `Nobody` (Layer-A floor). May be **empty** (selected groups expose
///   nothing → share nothing, default-closed).
///
/// `selected_group_ids` entries with no matching `Group` in `groups` are
/// ignored. `field_visibility` is the owner card's rules
/// ([`ContactCard::field_visibility`]).
pub(crate) fn resolve_exchange_allow(
    selected_group_ids: &[String],
    groups: &[Group],
    field_visibility: &VisibilityRules,
) -> Option<HashSet<String>> {
    if selected_group_ids.is_empty() {
        return None;
    }
    let mut allow: HashSet<String> = groups
        .iter()
        .filter(|g| selected_group_ids.iter().any(|id| id == g.id()))
        .flat_map(|g| g.visible_fields().iter().cloned())
        .collect();
    // Layer-A `Nobody` floor: even a stale group allow-list cannot widen past
    // a field the owner has marked private.
    allow.retain(|fid| !matches!(field_visibility.get(fid), FieldVisibility::Nobody));
    Some(allow)
}

// INLINE_TEST_REQUIRED: tests exercise the `pub(crate)` resolver, which is
// not reachable from a `tests/` integration directory.
#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a group with a known id and the given visible field ids.
    fn group(id: &str, visible: &[&str]) -> Group {
        Group::from_storage(
            id.to_string(),
            format!("group-{id}"),
            HashSet::new(),
            visible.iter().map(|s| s.to_string()).collect(),
            None,
            0,
            0,
        )
    }

    fn rules_all_everyone() -> VisibilityRules {
        VisibilityRules::new()
    }

    #[test]
    fn no_groups_selected_returns_none_share_all() {
        let groups = vec![group("g1", &["email"])];
        let allow = resolve_exchange_allow(&[], &groups, &rules_all_everyone());
        assert_eq!(allow, None, "no audience selected → share all (None)");
    }

    #[test]
    fn single_group_allows_exactly_its_visible_fields() {
        let groups = vec![group("g1", &["email"])];
        let allow = resolve_exchange_allow(&["g1".into()], &groups, &rules_all_everyone());
        assert_eq!(allow, Some(HashSet::from(["email".to_string()])));
    }

    #[test]
    fn field_not_in_any_selected_group_is_excluded() {
        // Group exposes email only; phone is absent from the allow-list.
        let groups = vec![group("g1", &["email"])];
        let allow = resolve_exchange_allow(&["g1".into()], &groups, &rules_all_everyone())
            .expect("groups selected → Some");
        assert!(allow.contains("email"), "email is in the group");
        assert!(!allow.contains("phone"), "phone is in no selected group");
    }

    #[test]
    fn multiple_groups_union_their_visible_fields() {
        let groups = vec![group("g1", &["email"]), group("g2", &["phone"])];
        let allow =
            resolve_exchange_allow(&["g1".into(), "g2".into()], &groups, &rules_all_everyone());
        assert_eq!(
            allow,
            Some(HashSet::from(["email".to_string(), "phone".to_string()]))
        );
    }

    #[test]
    fn nobody_default_field_excluded_even_if_group_lists_it() {
        // Stale group allow-list still lists `ssn`, but the owner marked it
        // Nobody — the floor must win.
        let groups = vec![group("g1", &["email", "ssn"])];
        let mut rules = VisibilityRules::new();
        rules.set_nobody("ssn");
        let allow = resolve_exchange_allow(&["g1".into()], &groups, &rules);
        assert_eq!(
            allow,
            Some(HashSet::from(["email".to_string()])),
            "Nobody floor drops ssn despite the group listing it"
        );
    }

    #[test]
    fn everyone_default_field_in_group_is_included() {
        // The floor only excludes Nobody; Everyone (default) stays in.
        let groups = vec![group("g1", &["email"])];
        let mut rules = VisibilityRules::new();
        rules.set_everyone("email");
        let allow = resolve_exchange_allow(&["g1".into()], &groups, &rules);
        assert_eq!(allow, Some(HashSet::from(["email".to_string()])));
    }

    #[test]
    fn empty_visible_fields_group_contributes_nothing_to_union() {
        let groups = vec![group("g1", &[]), group("g2", &["phone"])];
        let allow =
            resolve_exchange_allow(&["g1".into(), "g2".into()], &groups, &rules_all_everyone());
        assert_eq!(
            allow,
            Some(HashSet::from(["phone".to_string()])),
            "empty group adds nothing; g2 still contributes phone"
        );
    }

    #[test]
    fn all_selected_groups_empty_yields_empty_allow_not_share_all() {
        // Default-closed: selecting groups that expose nothing shares nothing,
        // NOT everything. This is the empty-set footgun the Option encoding
        // prevents (Some(∅) ≠ None).
        let groups = vec![group("g1", &[])];
        let allow = resolve_exchange_allow(&["g1".into()], &groups, &rules_all_everyone());
        assert_eq!(allow, Some(HashSet::new()), "Some(empty), not None");
    }

    #[test]
    fn unknown_selected_group_id_is_ignored() {
        // A selected id with no matching Group contributes nothing.
        let groups = vec![group("g1", &["email"])];
        let allow = resolve_exchange_allow(&["ghost".into()], &groups, &rules_all_everyone());
        assert_eq!(
            allow,
            Some(HashSet::new()),
            "no matching group → empty allow (share nothing), still Some"
        );
    }
}

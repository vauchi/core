// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::Vauchi;

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::component::ToggleItem;

/// What the per-contact visibility screen shows: the contact's name, the
/// groups they are in, and one switch per own-card entry.
pub(super) struct ContactVisibilityView {
    pub name: String,
    pub groups: Vec<String>,
    pub items: Vec<ToggleItem>,
}

/// Builds the screen from YOUR entries (#428: it listed the contact's own
/// fields). Each switch is `get_effective_field_visibility`, the rule that
/// filters what the contact receives; the reason follows that rule's order:
/// a per-contact switch, then the contact's groups, then groups the contact
/// is not in, then the all-contacts setting.
pub(super) fn contact_visibility_view(
    vauchi: &Vauchi,
    contact_id: &str,
    locale: Locale,
) -> Option<ContactVisibilityView> {
    let contact = vauchi.get_contact(contact_id).ok().flatten()?;
    let name = contact.display_name().to_string();
    let card = vauchi.own_card().ok().flatten()?;
    let overrides = vauchi
        .get_contact_visibility_overrides(contact_id)
        .unwrap_or_default();
    let contact_groups = vauchi
        .get_groups_for_contact(contact_id)
        .unwrap_or_default();
    let all_groups = vauchi.list_groups().unwrap_or_default();

    let items = card
        .fields()
        .iter()
        .map(|field| {
            let field_id = field.id();
            let selected = vauchi
                .get_effective_field_visibility(contact_id, field_id)
                .unwrap_or(false);
            let via_own_group = contact_groups.iter().find(|g| g.is_field_visible(field_id));
            let granting: Vec<&str> = all_groups
                .iter()
                .filter(|g| g.is_field_visible(field_id))
                .map(|g| g.name())
                .collect();
            let reason = match (overrides.get(field_id), via_own_group) {
                (Some(true), _) => get_string_with_args(
                    locale,
                    "contact_visibility.reason_override_on",
                    &[("name", &name)],
                ),
                (Some(false), _) => get_string_with_args(
                    locale,
                    "contact_visibility.reason_override_off",
                    &[("name", &name)],
                ),
                (None, Some(group)) => get_string_with_args(
                    locale,
                    "contact_visibility.reason_group",
                    &[("group", group.name())],
                ),
                (None, None) if !granting.is_empty() => get_string_with_args(
                    locale,
                    "contact_visibility.reason_other_groups",
                    &[("groups", &granting.join(", "))],
                ),
                (None, None) if selected => {
                    get_string(locale, "my_info_entry_detail.visible_to_all")
                }
                (None, None) => get_string_with_args(
                    locale,
                    "contact_visibility.reason_hidden",
                    &[("name", &name)],
                ),
            };
            ToggleItem {
                id: field_id.to_string(),
                label: field.label().to_string(),
                selected,
                subtitle: Some(reason),
                a11y: None,
                info_key: None,
            }
        })
        .collect();

    Some(ContactVisibilityView {
        name,
        groups: contact_groups
            .iter()
            .map(|g| g.name().to_string())
            .collect(),
        items,
    })
}

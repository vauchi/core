// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preview-shape wire types: the field/visibility/group-view types
//! consumed by `Component::CardPreview` and `Component::FieldList`.
//!
//! Phase-0 prep for the Wire Humble Tier 0 rename
//! (`2026-05-03-coreui-wire-humble-types`). The types here are
//! scheduled to become UI-shaped at the wire boundary —
//! `Field → Field`, `GroupCardView → PreviewVariant`,
//! `Component::CardPreview → Component::Preview`.

use serde::{Deserialize, Serialize};

use super::A11y;

/// A contact field as displayed in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    pub visibility: UiFieldVisibility,
    #[serde(default)]
    pub a11y: Option<A11y>,
}

/// UI-level field visibility state.
///
/// Named `UiFieldVisibility` to distinguish from `contact::FieldVisibility`
/// which is the storage-level visibility model.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiFieldVisibility {
    Shown,
    Hidden,
    Groups(Vec<String>),
}

/// How a card looks to a specific group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupCardView {
    pub group_name: String,
    pub display_name: String,
    pub visible_fields: Vec<Field>,
}

/// Compute the visible-fields list for a [`super::Component::CardPreview`].
///
/// - If `selected_group` is `Some` and matches a `GroupCardView`, returns
///   that group's `visible_fields` (already filtered by core's grouping
///   logic).
/// - Otherwise filters `fields` keeping only `Shown` and `Groups` variants
///   (drops `Hidden` so the preview never leaks fields the owner has
///   marked as hidden).
///
/// Used by [`super::Component::CardPreview`]'s `visible_fields` field so
/// that frontends never reproduce this filter in view code (ADR-021/043).
pub fn build_visible_fields(
    fields: &[Field],
    group_views: &[GroupCardView],
    selected_group: &Option<String>,
) -> Vec<Field> {
    // Selected group missing from group_views falls through to the filtered
    // `fields` list rather than passing raw fields — never leak `Hidden`
    // fields when the group lookup is stale.
    if let Some(group_name) = selected_group
        && let Some(view) = group_views.iter().find(|gv| &gv.group_name == group_name)
    {
        return view.visible_fields.clone();
    }
    fields
        .iter()
        .filter(|f| {
            matches!(
                f.visibility,
                UiFieldVisibility::Shown | UiFieldVisibility::Groups(_)
            )
        })
        .cloned()
        .collect()
}

// INLINE_TEST_REQUIRED: build_visible_fields is a pure helper with no public
// behavior surface beyond Component::CardPreview's visible_fields field;
// inline tests keep the filter logic + its invariants co-located with the
// helper so future changes to UiFieldVisibility variants surface here first.
#[cfg(test)]
mod build_visible_fields_tests {
    use super::*;

    fn field(id: &str, visibility: UiFieldVisibility) -> Field {
        Field {
            id: id.into(),
            field_type: "text".into(),
            label: id.into(),
            value: format!("value-{id}"),
            visibility,
            a11y: None,
        }
    }

    // @internal
    #[test]
    fn no_group_selected_keeps_shown_and_groups_drops_hidden() {
        let fields = vec![
            field("a", UiFieldVisibility::Shown),
            field("b", UiFieldVisibility::Hidden),
            field("c", UiFieldVisibility::Groups(vec!["work".into()])),
        ];
        let result = build_visible_fields(&fields, &[], &None);
        let ids: Vec<_> = result.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["a", "c"],
            "no group selected: keep Shown + Groups; drop Hidden"
        );
    }

    // @internal
    #[test]
    fn group_selected_and_present_returns_groupview_visible_fields() {
        let fields = vec![
            field("a", UiFieldVisibility::Shown),
            field("b", UiFieldVisibility::Hidden),
        ];
        let group_views = vec![GroupCardView {
            group_name: "work".into(),
            display_name: "Work".into(),
            visible_fields: vec![field("c", UiFieldVisibility::Shown)],
        }];
        let result = build_visible_fields(&fields, &group_views, &Some("work".into()));
        let ids: Vec<_> = result.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["c"],
            "group selected + present: must return that group's visible_fields, not the raw fields"
        );
    }

    // @internal
    #[test]
    fn group_selected_but_missing_falls_back_to_filtered_fields() {
        let fields = vec![
            field("a", UiFieldVisibility::Shown),
            field("b", UiFieldVisibility::Hidden),
        ];
        let group_views = vec![GroupCardView {
            group_name: "work".into(),
            display_name: "Work".into(),
            visible_fields: vec![field("c", UiFieldVisibility::Shown)],
        }];
        let result = build_visible_fields(&fields, &group_views, &Some("nonexistent".into()));
        let ids: Vec<_> = result.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["a"],
            "group missing: fall back to filtered fields (still drops Hidden), never raw"
        );
    }

    // @internal
    #[test]
    fn empty_inputs_return_empty() {
        assert!(build_visible_fields(&[], &[], &None).is_empty());
        assert!(build_visible_fields(&[], &[], &Some("work".into())).is_empty());
    }
}

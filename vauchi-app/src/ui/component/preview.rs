// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preview-shape wire types — `Field`, `UiFieldVisibility`,
//! `PreviewVariant`, `build_visible_fields` — consumed by
//! `Component::Preview` and `Component::FieldList`.
//!
//! Lives in `ui/component/preview.rs` (Wire Humble Tier 0 Phase 1):
//! UI-shaped names at the wire boundary, no domain leak. Engines map
//! their domain types (group views, locale variants, etc.) to
//! `PreviewVariant` at the wire boundary.

use serde::{Deserialize, Serialize};

use super::A11y;

/// A contact field as displayed in the UI.
///
/// `icon` carries a platform-neutral icon vocabulary name (see
/// [`icon_for_field_type`]) computed by core from `field_type`.
/// Frontends render this directly instead of duplicating the
/// `field_type` → icon switch in each renderer (ADR-021/043
/// Humble UI). Five frontends previously carried the same switch
/// (iOS×2, cli, tui, android); shipping `icon` on the wire collapses
/// that duplication and makes adding a new field type a single-file
/// change in core.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    /// Platform-neutral icon name (see [`icon_for_field_type`]).
    /// Frontends map this to their native icon system (SF Symbols /
    /// Material Symbols / their preferred glyph table).
    #[serde(default)]
    pub icon: String,
    pub visibility: UiFieldVisibility,
    #[serde(default)]
    pub a11y: Option<A11y>,
}

/// Map a `field_type` string to the platform-neutral icon name carried
/// on `Field.icon`.
///
/// The vocabulary follows the SF Symbols core set (`phone`, `envelope`,
/// `globe`, `mappin`, `at`, `gift`) which has direct equivalents in
/// Material Symbols and a documented mapping in every frontend's icon
/// table. Unknown field types fall back to `"tag"` (generic) so the
/// renderer always has something to draw.
///
/// Matching is case-insensitive so callers can pass either Debug-format
/// (`"Phone"`) or lowercase (`"phone"`) strings — both common in tree.
pub fn icon_for_field_type(field_type: &str) -> &'static str {
    match field_type.to_ascii_lowercase().as_str() {
        "phone" => "phone",
        "email" => "envelope",
        "website" => "globe",
        "address" => "mappin",
        "social" => "at",
        "birthday" => "gift",
        _ => "tag",
    }
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

/// One alternate look at a `Component::Preview` — a per-variant view
/// of the same content. Today only contact-card group views populate
/// this; future variants (per-locale views, accessibility variants,
/// previews-as-other-relationship) reuse the same shape.
///
/// Engines populate `variant_id` with whatever stable identifier they
/// know (group name, locale code, etc.); the renderer only matches it
/// against `Component::Preview.selected_variant` and never reads the
/// string for meaning.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreviewVariant {
    pub variant_id: String,
    pub display_name: String,
    pub visible_fields: Vec<Field>,
}

/// Compute the visible-fields list for a [`super::Component::Preview`].
///
/// - If `selected_variant` is `Some` and matches a `PreviewVariant`, returns
///   that group's `visible_fields` (already filtered by core's grouping
///   logic).
/// - Otherwise filters `fields` keeping only `Shown` and `Groups` variants
///   (drops `Hidden` so the preview never leaks fields the owner has
///   marked as hidden).
///
/// Used by [`super::Component::Preview`]'s `visible_fields` field so
/// that frontends never reproduce this filter in view code (ADR-021/043).
pub fn build_visible_fields(
    fields: &[Field],
    variants: &[PreviewVariant],
    selected_variant: &Option<String>,
) -> Vec<Field> {
    // Selected variant missing from variants falls through to the filtered
    // `fields` list rather than passing raw fields — never leak `Hidden`
    // fields when the variant lookup is stale.
    if let Some(selected_id) = selected_variant
        && let Some(view) = variants.iter().find(|v| &v.variant_id == selected_id)
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

/// Derive avatar initials from a display name: the first character of each
/// of the first two whitespace-separated words, uppercased.
///
/// Core owns this so frontends never recompute it (e.g. `displayName.take(1)`)
/// — the initials ride the wire on `Component::Preview`/`AvatarPreview`
/// (ADR-021/043 Humble UI). Empty/whitespace-only names yield `""`.
pub(crate) fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

// INLINE_TEST_REQUIRED: build_visible_fields is a pure helper with no public
// behavior surface beyond Component::Preview's visible_fields field;
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
            icon: icon_for_field_type("text").into(),
            visibility,
            a11y: None,
        }
    }

    // @internal
    #[test]
    fn icon_for_field_type_maps_known_types_case_insensitive() {
        // Title-case (Debug format of FieldType)
        assert_eq!(icon_for_field_type("Phone"), "phone");
        assert_eq!(icon_for_field_type("Email"), "envelope");
        assert_eq!(icon_for_field_type("Website"), "globe");
        assert_eq!(icon_for_field_type("Address"), "mappin");
        assert_eq!(icon_for_field_type("Social"), "at");
        assert_eq!(icon_for_field_type("Birthday"), "gift");
        // lowercase (typical EditableField input)
        assert_eq!(icon_for_field_type("phone"), "phone");
        assert_eq!(icon_for_field_type("email"), "envelope");
    }

    // @internal
    #[test]
    fn icon_for_field_type_unknown_falls_back_to_tag() {
        assert_eq!(icon_for_field_type("custom"), "tag");
        assert_eq!(icon_for_field_type("Custom"), "tag");
        assert_eq!(icon_for_field_type("anything_else"), "tag");
        assert_eq!(icon_for_field_type(""), "tag");
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
        let variants = vec![PreviewVariant {
            variant_id: "work".into(),
            display_name: "Work".into(),
            visible_fields: vec![field("c", UiFieldVisibility::Shown)],
        }];
        let result = build_visible_fields(&fields, &variants, &Some("work".into()));
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
        let variants = vec![PreviewVariant {
            variant_id: "work".into(),
            display_name: "Work".into(),
            visible_fields: vec![field("c", UiFieldVisibility::Shown)],
        }];
        let result = build_visible_fields(&fields, &variants, &Some("nonexistent".into()));
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

// INLINE_TEST_REQUIRED: initials() is a pub(crate) helper, not reachable from
// external tests/; co-locate its invariants with the implementation.
#[cfg(test)]
mod initials_tests {
    use super::initials;

    #[test]
    fn initials_single_word() {
        assert_eq!(initials("Alice"), "A");
    }

    #[test]
    fn initials_two_words() {
        assert_eq!(initials("Alice Smith"), "AS");
    }

    #[test]
    fn initials_three_words_takes_first_two() {
        assert_eq!(initials("Alice B Smith"), "AB");
    }

    #[test]
    fn initials_empty_string() {
        assert_eq!(initials(""), "");
    }

    #[test]
    fn initials_unicode() {
        assert_eq!(initials("Ägidius Ölmann"), "ÄÖ");
    }

    #[test]
    fn initials_extra_whitespace() {
        assert_eq!(initials("  Alice   Smith  "), "AS");
    }
}

// INLINE_TEST_REQUIRED: initials() is a pub(crate) helper, not reachable from
// external tests/; proptests co-located with the implementation.
#[cfg(test)]
mod initials_proptests {
    use super::initials;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn initials_never_panics(name in "\\PC*") {
            let result = initials(&name);
            // Unicode to_uppercase() can expand a single char to multiple,
            // so we only assert the result is valid UTF-8 (which String guarantees)
            // and that it equals its own uppercase form.
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }

        #[test]
        fn initials_are_uppercase(name in "[a-z]+ [a-z]+") {
            let result = initials(&name);
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }
    }
}

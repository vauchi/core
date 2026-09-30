// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preview-shape wire types — `Field`, `UiFieldVisibility` and
//! `PreviewVariant` — consumed by
//! `Component::Preview` and `Component::FieldList`.
//!
//! Lives in `ui/component/preview.rs` (Wire Humble Tier 0 Phase 1):
//! UI-shaped names at the wire boundary, no domain leak. Engines map
//! their domain types (group views, locale variants, etc.) to
//! `PreviewVariant` at the wire boundary.

use serde::{Deserialize, Serialize};
use vauchi_core::FieldType;

use crate::i18n::Locale;

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
    /// Core-resolved localized display copy for `visibility` (see
    /// [`visibility_label`]). Frontends render this verbatim; the
    /// `visibility` discriminant selects only native color.
    #[serde(default)]
    pub visibility_label: String,
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
///
/// The actual values are delegated to [`FieldType::icon`] in `vauchi-core`
/// so the vocabulary has a single source of truth.
pub fn icon_for_field_type(field_type: &str) -> &'static str {
    match field_type.to_ascii_lowercase().as_str() {
        "phone" => FieldType::Phone.icon(),
        "email" => FieldType::Email.icon(),
        "website" => FieldType::Website.icon(),
        "address" => FieldType::Address.icon(),
        "social" => FieldType::Social.icon(),
        "birthday" => FieldType::Birthday.icon(),
        "custom" => FieldType::Custom.icon(),
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
    Scopes(Vec<String>),
}

/// Resolve the display copy for a field's visibility state.
///
/// Frontends render the result verbatim (`Field.visibility_label`);
/// the `UiFieldVisibility` discriminant selects only native color
/// (ADR-043/044 — copy never derives from the discriminant in a
/// renderer).
pub fn visibility_label(visibility: &UiFieldVisibility, locale: Locale) -> String {
    use crate::i18n::get_string;
    match visibility {
        UiFieldVisibility::Shown => get_string(locale, "visibility.visible"),
        UiFieldVisibility::Hidden => get_string(locale, "visibility.hidden"),
        UiFieldVisibility::Scopes(scopes) if scopes.is_empty() => {
            get_string(locale, "visibility.no_groups")
        }
        UiFieldVisibility::Scopes(scopes) => scopes.join(", "),
    }
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

/// Derive avatar initials from a display name: the first character of each
/// of the first two whitespace-separated words, uppercased.
///
/// Core owns this so frontends never recompute it (e.g. `displayName.take(1)`)
/// — the initials ride the wire on `Component::Preview`/`ImageCircle`
/// (ADR-021/043 Humble UI). Empty/whitespace-only names yield `""`.
pub(crate) fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

// INLINE_TEST_REQUIRED: initials() is a pub(crate) helper, not reachable from
// external tests/; co-locate its invariants with the implementation.
#[cfg(test)]
mod initials_tests {
    use super::initials;

    // @internal
    #[test]
    fn initials_single_word() {
        assert_eq!(initials("Alice"), "A");
    }

    // @internal
    #[test]
    fn initials_two_words() {
        assert_eq!(initials("Alice Smith"), "AS");
    }

    // @internal
    #[test]
    fn initials_three_words_takes_first_two() {
        assert_eq!(initials("Alice B Smith"), "AB");
    }

    // @internal
    #[test]
    fn initials_empty_string() {
        assert_eq!(initials(""), "");
    }

    // @internal
    #[test]
    fn initials_unicode() {
        assert_eq!(initials("Ägidius Ölmann"), "ÄÖ");
    }

    // @internal
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
        // @internal
        #[test]
        fn initials_never_panics(name in "\\PC*") {
            let result = initials(&name);
            // Unicode to_uppercase() can expand a single char to multiple,
            // so we only assert the result is valid UTF-8 (which String guarantees)
            // and that it equals its own uppercase form.
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }

        // @internal
        #[test]
        fn initials_are_uppercase(name in "[a-z]+ [a-z]+") {
            let result = initials(&name);
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }
    }
}

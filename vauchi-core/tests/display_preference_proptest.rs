// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property tests for display name resolution (CC-13).
//!
//! Verifies that resolved display name never panics and always returns
//! a non-empty string under random input sequences.

use proptest::prelude::*;
use vauchi_core::contact::display::{DisplayPreference, NameVariant, resolve_display_name};

fn display_name_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,49}"
        .prop_map(|s| s.trim().to_string())
        .prop_filter("non-empty", |s| !s.is_empty())
}

fn source_label_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),
        Just("Work".to_string()),
        Just("Family".to_string()),
        Just("Friends".to_string()),
        "[a-zA-Z]{1,20}",
    ]
}

fn variant_strategy() -> impl Strategy<Value = NameVariant> {
    (
        source_label_strategy(),
        display_name_strategy(),
        any::<bool>(),
    )
        .prop_map(|(label, name, has_avatar)| NameVariant {
            source_label: label,
            name,
            has_avatar,
            updated_at: 1000,
        })
}

proptest! {
    #[test]
    fn resolved_name_never_empty(
        default_name in display_name_strategy(),
        variants in prop::collection::vec(variant_strategy(), 0..5),
        nickname in proptest::option::of(display_name_strategy()),
    ) {
        // Test all three preference types
        let result_default = resolve_display_name(
            &default_name, &DisplayPreference::CardDefault, &variants, nickname.as_deref(),
        );
        prop_assert!(!result_default.is_empty(), "CardDefault result must not be empty");

        let result_custom = resolve_display_name(
            &default_name, &DisplayPreference::Custom, &variants, nickname.as_deref(),
        );
        prop_assert!(!result_custom.is_empty(), "Custom result must not be empty");

        if let Some(v) = variants.first() {
            let result_variant = resolve_display_name(
                &default_name,
                &DisplayPreference::CardVariant { source_label: v.source_label.clone() },
                &variants,
                nickname.as_deref(),
            );
            prop_assert!(!result_variant.is_empty(), "CardVariant result must not be empty");
        }
    }

    #[test]
    fn card_default_always_returns_default_name(
        default_name in display_name_strategy(),
        variants in prop::collection::vec(variant_strategy(), 0..5),
        nickname in proptest::option::of(display_name_strategy()),
    ) {
        let result = resolve_display_name(
            &default_name, &DisplayPreference::CardDefault, &variants, nickname.as_deref(),
        );
        prop_assert_eq!(result, default_name);
    }

    #[test]
    fn custom_with_nickname_returns_nickname(
        default_name in display_name_strategy(),
        nickname in display_name_strategy(),
    ) {
        let result = resolve_display_name(
            &default_name, &DisplayPreference::Custom, &[], Some(&nickname),
        );
        prop_assert_eq!(result, nickname);
    }

    #[test]
    fn custom_without_nickname_falls_back(
        default_name in display_name_strategy(),
    ) {
        let result = resolve_display_name(
            &default_name, &DisplayPreference::Custom, &[], None,
        );
        prop_assert_eq!(result, default_name, "Custom without nickname must fall back to default");
    }

    #[test]
    fn card_variant_missing_label_falls_back(
        default_name in display_name_strategy(),
        variants in prop::collection::vec(variant_strategy(), 0..5),
    ) {
        let result = resolve_display_name(
            &default_name,
            &DisplayPreference::CardVariant { source_label: "nonexistent_label_xyz".to_string() },
            &variants,
            None,
        );
        prop_assert_eq!(result, default_name, "Missing variant must fall back to default");
    }
}

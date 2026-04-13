// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for display name resolution logic.
//!
//! @scenario: contacts_management.feature - Display name resolution invariants

use proptest::prelude::*;
use vauchi_core::contact::display::{DisplayNamePreference, SharedName, resolve_display_name};

fn shared_name_strategy() -> impl Strategy<Value = SharedName> {
    ("[a-zA-Z][a-zA-Z0-9 ]{0,29}", any::<bool>())
        .prop_map(|(name, is_primary)| SharedName {
            name: name.trim().to_string(),
            is_primary,
            updated_at: 1000,
        })
        .prop_filter("non-empty name after trim", |n| !n.name.is_empty())
}

fn display_name_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9]{1,29}".prop_map(|s| s)
}

/// Build a vec of SharedName with exactly one is_primary=true entry.
fn shared_names_with_primary() -> impl Strategy<Value = Vec<SharedName>> {
    (
        display_name_strategy(),
        prop::collection::vec(shared_name_strategy(), 0..5),
    )
        .prop_map(|(primary_name, mut names)| {
            for n in &mut names {
                n.is_primary = false;
            }
            names.insert(
                0,
                SharedName {
                    name: primary_name,
                    is_primary: true,
                    updated_at: 1000,
                },
            );
            names
        })
}

proptest! {
    /// Resolved name is never empty — regardless of preference, shared names, or nickname.
    #[test]
    fn resolved_name_never_empty(
        names in shared_names_with_primary(),
        nickname in proptest::option::of("[a-zA-Z]{1,30}"),
    ) {
        let default_name = "DefaultContact";
        for pref in &[
            DisplayNamePreference::Primary,
            DisplayNamePreference::Custom,
            DisplayNamePreference::SharedName { name: names[0].name.clone() },
        ] {
            let result = resolve_display_name(
                default_name,
                pref,
                &names,
                nickname.as_deref(),
            );
            prop_assert!(
                !result.is_empty(),
                "resolve_display_name must never return empty; pref={pref:?}, nick={nickname:?}"
            );
        }
    }

    /// Primary preference always returns the is_primary=true shared name.
    #[test]
    fn primary_returns_primary_name(
        names in shared_names_with_primary(),
        nickname in proptest::option::of("[a-zA-Z]{1,30}"),
    ) {
        let primary_name = names
            .iter()
            .find(|n| n.is_primary)
            .map(|n| n.name.clone())
            .unwrap();

        let result = resolve_display_name(
            "DefaultContact",
            &DisplayNamePreference::Primary,
            &names,
            nickname.as_deref(),
        );
        prop_assert_eq!(
            &result,
            &primary_name,
            "Primary pref must return the primary shared name"
        );
    }

    /// Custom pref + nickname returns the nickname.
    #[test]
    fn custom_with_nickname_returns_nickname(
        names in shared_names_with_primary(),
        nickname in "[a-zA-Z]{1,30}",
    ) {
        let result = resolve_display_name(
            "DefaultContact",
            &DisplayNamePreference::Custom,
            &names,
            Some(&nickname),
        );
        prop_assert_eq!(
            &result,
            &nickname,
            "Custom pref with nickname must return the nickname"
        );
    }

    /// Custom pref without a nickname falls back to primary shared name.
    #[test]
    fn custom_without_nickname_falls_back_to_primary(
        names in shared_names_with_primary(),
    ) {
        let primary_name = names
            .iter()
            .find(|n| n.is_primary)
            .map(|n| n.name.clone())
            .unwrap();

        let result = resolve_display_name(
            "DefaultContact",
            &DisplayNamePreference::Custom,
            &names,
            None,
        );
        prop_assert_eq!(
            &result,
            &primary_name,
            "Custom pref without nickname must fall back to primary shared name"
        );
    }

    /// SharedName pref for a name not in the set falls back to primary.
    #[test]
    fn shared_name_missing_falls_back_to_primary(
        names in shared_names_with_primary(),
    ) {
        let primary_name = names
            .iter()
            .find(|n| n.is_primary)
            .map(|n| n.name.clone())
            .unwrap();

        // Use a name that cannot exist in the set (starts with digits — outside strategy range)
        let missing = "00000nonexistent";
        let result = resolve_display_name(
            "DefaultContact",
            &DisplayNamePreference::SharedName {
                name: missing.to_string(),
            },
            &names,
            None,
        );
        prop_assert_eq!(
            &result,
            &primary_name,
            "SharedName pref for missing name must fall back to primary"
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for find_group_fuzzy API
//! Trace: ADR-021 Tier 1 — find_group_fuzzy

use vauchi_core::api::*;
use vauchi_core::*;

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

#[test]
fn test_find_label_fuzzy_matches_case_insensitive_name() {
    let wb = create_test_vauchi();

    let label = wb.create_group("Family").unwrap();

    let found = wb.find_group_fuzzy("family").unwrap();

    assert!(
        found.is_some(),
        "should find 'Family' when searching for 'family'"
    );
    let found = found.unwrap();
    assert_eq!(found.id(), label.id());
    assert_eq!(found.name(), "Family");
}

#[test]
fn test_find_label_fuzzy_matches_exact_name() {
    let wb = create_test_vauchi();

    let label = wb.create_group("Professional").unwrap();

    let found = wb.find_group_fuzzy("Professional").unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id(), label.id());
}

#[test]
fn test_find_label_fuzzy_matches_id_prefix() {
    let wb = create_test_vauchi();

    let label = wb.create_group("Work").unwrap();
    let label_id = label.id().to_string();

    // Use the first 8 characters of the label's ID
    let prefix = &label_id[..8];
    let found = wb.find_group_fuzzy(prefix).unwrap();

    assert!(
        found.is_some(),
        "should find label by ID prefix '{}'",
        prefix
    );
    assert_eq!(found.unwrap().id(), label_id);
}

#[test]
fn test_find_label_fuzzy_returns_none_for_no_match() {
    let wb = create_test_vauchi();

    wb.create_group("Family").unwrap();
    wb.create_group("Work").unwrap();

    let found = wb.find_group_fuzzy("zzz_no_match").unwrap();

    assert!(found.is_none(), "should return None for non-matching query");
}

#[test]
fn test_find_label_fuzzy_returns_none_when_no_labels_exist() {
    let wb = create_test_vauchi();

    let found = wb.find_group_fuzzy("anything").unwrap();

    assert!(found.is_none(), "should return None when no labels exist");
}

#[test]
fn test_find_label_fuzzy_prefers_name_match_over_id_prefix() {
    let wb = create_test_vauchi();

    // Create two labels
    let label1 = wb.create_group("Friends").unwrap();
    wb.create_group("Colleagues").unwrap();

    // Search by name should find the right one
    let found = wb.find_group_fuzzy("friends").unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id(), label1.id());
}

#[test]
fn test_find_label_fuzzy_mixed_case_name() {
    let wb = create_test_vauchi();

    let label = wb.create_group("Close Friends").unwrap();

    let found = wb.find_group_fuzzy("CLOSE FRIENDS").unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id(), label.id());
}

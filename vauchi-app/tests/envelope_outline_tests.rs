// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The envelope outline is the structure of a command batch with every
//! text replaced by its length: what a shell receives (node kinds, ids,
//! roles, tones, order), without what the user typed. Two shells' outlines
//! of the same surface are diffed to tell a Core difference from a shell
//! one (vauchi/private#1206, UI/UX clarity addendum step 2). It must never
//! carry a name, a number or a card value, so a dev build may log it.

use std::collections::BTreeMap;

use vauchi_app::ui::{ScreenCatalogFixture, command_outline};

fn catalog() -> ScreenCatalogFixture {
    serde_json::from_str(vauchi_app::ui::screen_catalog_fixture_json())
        .expect("checked-in screen catalog parses")
}

fn entry(code_id: &str) -> vauchi_app::ui::ScreenCatalogEntry {
    catalog()
        .screens
        .into_iter()
        .find(|s| s.code_id == code_id)
        .unwrap_or_else(|| panic!("catalog has {code_id}"))
}

fn flatten(value: &serde_json::Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten(v, &format!("{path}/{k}"), out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                flatten(v, &format!("{path}[{i}]"), out);
            }
        }
        serde_json::Value::String(s) => out.push((path.to_owned(), s.clone())),
        other => out.push((path.to_owned(), other.to_string())),
    }
}

// @scenario: generic_presentation_protocol :: Commands are complete
// @internal
#[test]
fn an_outline_keeps_the_structure_and_replaces_every_text_with_its_length() {
    let my_info = entry("my_info");
    let outline = command_outline(&my_info.commands);

    let mut leaves = Vec::new();
    flatten(&outline, "", &mut leaves);
    let titles: Vec<&(String, String)> = leaves
        .iter()
        .filter(|(path, _)| path.ends_with("/title"))
        .collect();
    assert!(!titles.is_empty(), "the surface and its rows carry titles");
    for (path, value) in &titles {
        assert!(
            value.starts_with("len:"),
            "{path} should be a length marker, was {value:?}"
        );
    }
    let surface_ids: Vec<&(String, String)> = leaves
        .iter()
        .filter(|(path, _)| path.ends_with("/surface_id"))
        .collect();
    assert_eq!(
        surface_ids.first().map(|(_, v)| v.as_str()),
        Some("my_info"),
        "ids stay verbatim: the diff is keyed on them"
    );
    assert!(
        leaves
            .iter()
            .any(|(path, _)| path.contains("/ReplaceSurface/")),
        "command variants stay as object keys"
    );
    assert!(
        leaves.iter().any(|(path, _)| path.contains("/List/rows[")),
        "node kinds and row order stay"
    );
}

// @internal
#[test]
fn an_outline_of_the_whole_catalog_carries_no_seeded_name_or_value() {
    let fixture = catalog();
    let seeded = [
        "Ada Lovelace",
        "Grace Hopper",
        "Alan Turing",
        "7946",
        "analytical.example.org",
        "St James",
    ];
    for screen in &fixture.screens {
        let text = command_outline(&screen.commands).to_string();
        for needle in seeded {
            assert!(
                !text.contains(needle),
                "{} outline leaks {needle:?}",
                screen.code_id
            );
        }
    }
}

// @internal
#[test]
fn the_same_batch_has_the_same_outline_every_time() {
    let my_info = entry("my_info");
    let first = command_outline(&my_info.commands);
    let second = command_outline(&my_info.commands);
    assert_eq!(first, second);
    assert_ne!(
        command_outline(&entry("my_info-empty").commands),
        first,
        "a different batch has a different outline"
    );
}

/// The Core side of the two-shell diff: one outline per catalog screen,
/// written on demand like the catalog itself.
// @internal
#[test]
#[ignore = "writes fixtures/journeys/screen_outline_v1.json from the checked-in catalog"]
fn regenerate_outline_fixture() {
    let fixture = catalog();
    let outlines: BTreeMap<String, serde_json::Value> = fixture
        .screens
        .iter()
        .map(|s| (s.code_id.clone(), command_outline(&s.commands)))
        .collect();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/journeys/screen_outline_v1.json"
    );
    let json = serde_json::to_string_pretty(&outlines).expect("outlines serialize");
    std::fs::write(path, format!("{json}\n")).expect("outline fixture written");
}

// @internal
#[test]
fn the_checked_in_outline_fixture_matches_the_checked_in_catalog() {
    let checked_in: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(include_str!("../fixtures/journeys/screen_outline_v1.json"))
            .expect("outline fixture parses");
    let fixture = catalog();
    let fresh: BTreeMap<String, serde_json::Value> = fixture
        .screens
        .iter()
        .map(|s| (s.code_id.clone(), command_outline(&s.commands)))
        .collect();
    let drifted: Vec<&String> = fresh
        .iter()
        .filter(|(id, outline)| checked_in.get(*id) != Some(outline))
        .map(|(id, _)| id)
        .collect();
    assert!(
        drifted.is_empty(),
        "outline fixture drifted for {drifted:?}; run the ignored regenerate_outline_fixture test"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the tags-list screen
//! renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{TagSummary, TagsEngine, WorkflowEngine};

fn sample_tags() -> Vec<TagSummary> {
    vec![TagSummary {
        id: "t1".into(),
        name: "Friends".into(),
        member_count: 1,
    }]
}

/// The first row action label — the "promote to group" affordance.
fn promote_label(locale: Locale) -> String {
    let engine = TagsEngine::new(sample_tags()).with_locale(locale);
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::List { items, .. } => items
                .iter()
                .find_map(|i| i.actions.first().map(|a| a.label.clone())),
            _ => None,
        })
        .expect("tag rows carry a promote action")
}

// @scenario: tags-list :: screen renders in the active locale
// @internal
#[test]
fn tags_list_renders_the_active_locale() {
    load_german();
    assert_translated(
        "promote-to-group action",
        &promote_label(Locale::German),
        &promote_label(Locale::English),
    );
}

// @scenario: tags-list :: English copy unchanged (regression pin)
// English is the source language and ships bundled, so pinning it here
// couples nothing external.
// @internal
#[test]
fn tags_list_english_copy_unchanged() {
    let engine = TagsEngine::new(sample_tags());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Tags");
    let has_singular = screen.components.iter().any(|c| {
        matches!(c, vauchi_app::ui::Component::List { items, .. }
            if items.first().and_then(|i| i.subtitle.as_deref()) == Some("1 contact"))
    });
    assert!(has_singular, "singular member count must read '1 contact'");
}

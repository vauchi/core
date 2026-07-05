// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-15 (`2026-07-04-m3-i18n-threading-plan`): tags_list.rs renders
//! in the user's locale. Keys under `tags_list.*` (locales!114). Exact
//! German assertion per CC-03; closes out the M3 S5 sweep entirely.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{TagSummary, TagsEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn sample_tags() -> Vec<TagSummary> {
    vec![TagSummary {
        id: "t1".into(),
        name: "Friends".into(),
        member_count: 1,
    }]
}

// @scenario: tags-list :: screen renders in the active locale
// @internal
#[test]
fn tags_list_renders_german() {
    load_german();
    let engine = TagsEngine::new(sample_tags()).with_locale(Locale::German);
    let screen = engine.current_screen();
    let has_promote_label = screen.components.iter().any(|c| {
        matches!(c, vauchi_app::ui::Component::List { items, .. }
            if items.iter().any(|i| i.actions.iter().any(|a| a.label == "Zu Gruppe befördern")))
    });
    assert!(has_promote_label, "Promote action must render in German");
}

// @scenario: tags-list :: English copy unchanged (regression pin)
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

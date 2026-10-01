// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Tags screen (#467, design pass #419 item 9): it says what tags are,
//! says how to make one when there are none, and its Delete reads as the
//! destructive action it is.

use vauchi_app::ui::{Component, ScreenModel, TagSummary, TagsEngine, WorkflowEngine};

const INTRO: &str = "Private labels, only on your devices. A tag that grows can become a group.";
const EMPTY: &str = "No tags yet. Tag a contact from their page; tags stay on your devices only.";

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn climbing() -> TagSummary {
    TagSummary {
        id: "t1".into(),
        name: "climbing".into(),
        member_count: 5,
    }
}

// @internal
#[test]
fn tags_say_what_they_are() {
    let screen = TagsEngine::new(vec![climbing()]).current_screen();
    assert_eq!(texts(&screen), [INTRO]);
}

// @internal
#[test]
fn no_tags_says_how_to_make_one() {
    let screen = TagsEngine::new(vec![]).current_screen();
    assert_eq!(texts(&screen), [INTRO, EMPTY]);
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::List { .. })),
        "no empty list under the explanation"
    );
}

// @internal
#[test]
fn delete_reads_as_destructive() {
    let screen = TagsEngine::new(vec![climbing()]).current_screen();
    let delete = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "tags" => items[0]
                .actions
                .iter()
                .find(|a| a.id == "request_delete")
                .cloned(),
            _ => None,
        })
        .expect("a Delete action on the row");
    assert!(delete.destructive);
}

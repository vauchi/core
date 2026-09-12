// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `my_info.rs` — extracted to keep the engine file
//! under the src size limit. Loaded via `#[path]`; stays a unit-test
//! child module (private `MyInfoViewMode` access preserved).

// INLINE_TEST_REQUIRED: extracted from my_info.rs via #[path]; MyInfoViewMode
// is module-private and cannot be tested from external tests/.
use super::*;

#[test]
fn test_my_info_has_preview_as_action_in_entry_view() {
    let engine = MyInfoEngine::new(MyInfoProgress::default());
    let screen = engine.current_screen();

    let action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "preview-as-picker");
    assert!(
        action.is_some(),
        "MyInfo (EntryView) should have 'preview-as-picker' action"
    );
    assert_eq!(action.unwrap().label, "Preview as...");
}

#[test]
fn test_my_info_has_preview_as_action_in_group_view() {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_view_mode(MyInfoViewMode::GroupView { selected_tab: 0 });
    let screen = engine.current_screen();

    let action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "preview-as-picker");
    assert!(
        action.is_some(),
        "MyInfo (GroupView) should have 'preview-as-picker' action"
    );
}

#[test]
fn test_my_info_preview_mode_has_no_preview_as_picker_action() {
    let engine =
        MyInfoEngine::new(MyInfoProgress::default()).with_view_mode(MyInfoViewMode::PreviewAs {
            contact_name: "Alice".into(),
        });
    let screen = engine.current_screen();

    let action = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "preview-as-picker");
    assert!(
        action.is_none(),
        "MyInfo in PreviewAs mode should NOT have 'preview-as-picker' action"
    );
}

#[test]
fn test_preview_as_picker_returns_show_contact_picker() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "preview-as-picker".into(),
    });
    assert_eq!(result, ActionResult::ShowContactPicker);
}

fn caption_content(screen: &ScreenModel, id: &str) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::Text {
            a11y: None,
            id: cid,
            content,
            ..
        } if cid == id => Some(content.clone()),
        _ => None,
    })
}

// @internal
#[test]
fn test_my_info_emits_last_sync_caption() {
    // 5 minutes ago — format_relative_time renders "5 minutes ago"
    let now = 1_700_000_000u64;
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_last_sync_seconds(Some(now - 5 * 60))
        .with_now_seconds(now);
    let screen = engine.current_screen();
    assert_eq!(
        caption_content(&screen, "last_sync_caption").as_deref(),
        Some("Last synced 5 minutes ago"),
    );
}

// @internal
#[test]
fn test_my_info_omits_last_sync_caption_when_none() {
    let engine = MyInfoEngine::new(MyInfoProgress::default()).with_now_seconds(1_700_000_000);
    let screen = engine.current_screen();
    assert!(caption_content(&screen, "last_sync_caption").is_none());
}

// @internal
#[test]
fn test_my_info_preview_mode_omits_sync_status_captions() {
    let now = 1_700_000_000u64;
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_pending_updates(5)
        .with_last_sync_seconds(Some(now - 60))
        .with_now_seconds(now)
        .with_view_mode(MyInfoViewMode::PreviewAs {
            contact_name: "Alice".into(),
        });
    let screen = engine.current_screen();
    assert!(
        caption_content(&screen, "pending_updates_caption").is_none(),
        "PreviewAs renders the card as the contact sees it — owner-only sync status must not leak"
    );
    assert!(caption_content(&screen, "last_sync_caption").is_none());
    assert!(
        screen.subtitle.is_none(),
        "the sharing summary is owner-only and must not leak into the preview"
    );
}

fn own_field(id: &str, value: &str, label: &str, groups: &[&str], shown: bool) -> OwnFieldInfo {
    OwnFieldInfo {
        field_id: id.into(),
        field_type: "Phone".into(),
        label: label.into(),
        value: value.into(),
        visible_groups: groups.iter().map(|g| (*g).to_string()).collect(),
        contact_count: 0,
        shown,
    }
}

fn own_entries(screen: &ScreenModel) -> Vec<Item> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "own_entries" => Some(items.clone()),
            _ => None,
        })
        .expect("My Card renders its entries as the own_entries List")
}

fn card_with(fields: Vec<OwnFieldInfo>) -> MyInfoEngine {
    MyInfoEngine::new(MyInfoProgress::default()).with_own_card("Tessa Urech".into(), fields)
}

// @internal
#[test]
fn entries_render_the_value_as_title_and_the_label_as_subtitle() {
    let screen = card_with(vec![own_field(
        "f1",
        "+41 79 000 00 00",
        "Mobile",
        &[],
        true,
    )])
    .current_screen();
    let items = own_entries(&screen);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "f1");
    assert_eq!(items[0].name, "+41 79 000 00 00");
    assert_eq!(items[0].subtitle.as_deref(), Some("Mobile"));
}

// @internal
#[test]
fn entry_shown_to_every_contact_carries_the_everyone_chip() {
    let screen = card_with(vec![own_field(
        "f1",
        "tessa@example.org",
        "Email",
        &[],
        true,
    )])
    .current_screen();
    assert_eq!(own_entries(&screen)[0].status.as_deref(), Some("Everyone"));
}

// @internal
#[test]
fn entry_granted_by_groups_carries_the_group_names_chip() {
    let screen = card_with(vec![own_field(
        "f1",
        "+41 44 000 00 00",
        "Work",
        &["Family", "Cycling club"],
        false,
    )])
    .current_screen();
    assert_eq!(
        own_entries(&screen)[0].status.as_deref(),
        Some("Family, Cycling club")
    );
}

// @internal
#[test]
fn entry_hidden_from_every_contact_carries_the_hidden_chip() {
    let screen =
        card_with(vec![own_field("f1", "1815-12-10", "Birthday", &[], false)]).current_screen();
    assert_eq!(own_entries(&screen)[0].status.as_deref(), Some("Hidden"));
}

// @internal
#[test]
fn entry_without_a_label_falls_back_to_its_field_type() {
    let screen =
        card_with(vec![own_field("f1", "+41 79 000 00 00", "", &[], true)]).current_screen();
    assert_eq!(own_entries(&screen)[0].subtitle.as_deref(), Some("Phone"));
}

// @internal
#[test]
fn selecting_an_entry_opens_its_detail() {
    let mut engine = card_with(vec![own_field(
        "f1",
        "+41 79 000 00 00",
        "Mobile",
        &[],
        true,
    )]);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "own_entries".into(),
        item_id: "f1".into(),
    });
    assert_eq!(
        result,
        ActionResult::OpenEntryDetail {
            field_id: "f1".into()
        }
    );
}

// @internal
#[test]
fn sharing_summary_joins_contact_count_and_pending_updates() {
    let screen = MyInfoEngine::new(MyInfoProgress::default())
        .with_contact_count(7)
        .with_pending_updates(1)
        .current_screen();
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Shared with 7 contacts · 1 pending update")
    );
}

// @internal
#[test]
fn sharing_summary_uses_the_singular_for_one_contact() {
    let screen = MyInfoEngine::new(MyInfoProgress::default())
        .with_contact_count(1)
        .current_screen();
    assert_eq!(screen.subtitle.as_deref(), Some("Shared with 1 contact"));
}

// @internal
#[test]
fn sharing_summary_omits_pending_updates_when_none_are_queued() {
    let screen = MyInfoEngine::new(MyInfoProgress::default())
        .with_contact_count(3)
        .with_pending_updates(0)
        .current_screen();
    assert_eq!(screen.subtitle.as_deref(), Some("Shared with 3 contacts"));
    assert!(caption_content(&screen, "pending_updates_caption").is_none());
}

// @internal
#[test]
fn pending_updates_live_in_the_sharing_summary_not_a_caption() {
    let screen = MyInfoEngine::new(MyInfoProgress::default())
        .with_pending_updates(3)
        .current_screen();
    assert!(caption_content(&screen, "pending_updates_caption").is_none());
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Shared with 0 contacts · 3 pending updates")
    );
}

// @internal
#[test]
fn entry_view_offers_group_view_and_preview_before_add_entry() {
    let screen = MyInfoEngine::new(MyInfoProgress::default()).current_screen();
    let ids: Vec<&str> = screen
        .contextual_actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, ["toggle_view", "preview-as-picker", "add_field"]);
    let labels: Vec<&str> = screen
        .contextual_actions
        .iter()
        .map(|a| a.label.as_str())
        .collect();
    assert_eq!(labels, ["Group View", "Preview as...", "Add Entry"]);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `my_info.rs` — extracted to keep the engine file
//! under the src size limit. Loaded via `#[path]`; stays a unit-test
//! child module (private `MyInfoViewMode` access preserved).

// INLINE_TEST_REQUIRED: extracted from my_info.rs via #[path]; MyInfoViewMode
// is module-private and cannot be tested from external tests/.
use super::*;

fn preview_as_row(screen: &ScreenModel) -> Option<&ActionListItem> {
    screen.components.iter().find_map(|c| match c {
        Component::ActionList { id, items } if id == "preview_as" => items.first(),
        _ => None,
    })
}

type ViewSwitch<'a> = (Option<&'a str>, Vec<(&'a str, &'a str)>);

fn view_switch(screen: &ScreenModel) -> Option<ViewSwitch<'_>> {
    screen.components.iter().find_map(|c| match c {
        Component::Dropdown {
            id,
            selected,
            options,
            ..
        } if id == "view_mode" => Some((
            selected.as_deref(),
            options
                .iter()
                .map(|o| (o.id.as_str(), o.label.as_str()))
                .collect(),
        )),
        _ => None,
    })
}

// The MyCard artboard draws Group View and Preview as beside the header;
// as secondary context actions every shell folded them into the Actions
// sheet (problems/2026-09-13-device-walk-diverges-from-canvas, plan 2).
// @internal
#[test]
fn view_switches_sit_in_the_card_body_not_the_actions_sheet() {
    for engine in [
        MyInfoEngine::new(MyInfoProgress::default()),
        MyInfoEngine::new(MyInfoProgress::default())
            .with_view_mode(MyInfoViewMode::GroupView { selected_tab: 0 }),
    ] {
        let screen = engine.current_screen();

        let ids: Vec<&str> = screen
            .contextual_actions
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(ids, ["add_field"]);
        let preview = preview_as_row(&screen).expect("Preview as is a body row");
        assert_eq!(
            (preview.id.as_str(), preview.label.as_str()),
            ("preview-as-picker", "Preview as...")
        );
    }
}

// @internal
#[test]
fn the_view_switch_offers_entry_and_group_view_with_the_current_one_selected() {
    let entry = MyInfoEngine::new(MyInfoProgress::default()).current_screen();
    assert_eq!(
        view_switch(&entry),
        Some((
            Some("entries"),
            vec![("entries", "Entry View"), ("groups", "Group View")]
        ))
    );

    let group = MyInfoEngine::new(MyInfoProgress::default())
        .with_view_mode(MyInfoViewMode::GroupView { selected_tab: 0 })
        .current_screen();
    assert_eq!(
        view_switch(&group).map(|(selected, _)| selected),
        Some(Some("groups"))
    );
}

// @internal
#[test]
fn choosing_a_view_switches_between_entries_and_groups() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default());
    let choose = |engine: &mut MyInfoEngine, item_id: &str| {
        engine.handle_action(UserAction::ListItemSelected {
            component_id: "view_mode".into(),
            item_id: item_id.into(),
        })
    };

    let ActionResult::UpdateScreen(groups) = choose(&mut engine, "groups") else {
        panic!("choosing a view re-renders MyCard");
    };
    assert_eq!(
        view_switch(&groups).map(|(selected, _)| selected),
        Some(Some("groups"))
    );
    assert_eq!(
        engine.view_mode,
        MyInfoViewMode::GroupView { selected_tab: 0 }
    );

    let ActionResult::UpdateScreen(entries) = choose(&mut engine, "entries") else {
        panic!("choosing a view re-renders MyCard");
    };
    assert_eq!(
        view_switch(&entries).map(|(selected, _)| selected),
        Some(Some("entries"))
    );
    assert_eq!(engine.view_mode, MyInfoViewMode::EntryView);
}

// @internal
#[test]
fn an_unknown_view_leaves_the_card_as_it_was() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_view_mode(MyInfoViewMode::GroupView { selected_tab: 0 });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "view_mode".into(),
        item_id: "timeline".into(),
    });
    assert_eq!(
        engine.view_mode,
        MyInfoViewMode::GroupView { selected_tab: 0 }
    );
}

// @internal
#[test]
fn preview_mode_offers_no_view_switches() {
    let screen = MyInfoEngine::new(MyInfoProgress::default())
        .with_view_mode(MyInfoViewMode::PreviewAs {
            contact_name: "Alice".into(),
        })
        .current_screen();
    assert!(preview_as_row(&screen).is_none());
    assert!(view_switch(&screen).is_none());
}

// @internal
#[test]
fn choosing_preview_as_opens_the_contact_picker() {
    let mut engine = MyInfoEngine::new(MyInfoProgress::default());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "preview_as".into(),
        item_id: "preview-as-picker".into(),
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

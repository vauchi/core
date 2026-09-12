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
fn test_my_info_emits_pending_updates_caption() {
    let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(3);
    let screen = engine.current_screen();
    assert_eq!(
        caption_content(&screen, "pending_updates_caption").as_deref(),
        Some("3 pending updates"),
    );
}

// @internal
#[test]
fn test_my_info_pending_updates_caption_uses_singular_for_one() {
    let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(1);
    let screen = engine.current_screen();
    assert_eq!(
        caption_content(&screen, "pending_updates_caption").as_deref(),
        Some("1 pending update"),
    );
}

// @internal
#[test]
fn test_my_info_omits_pending_updates_caption_when_zero() {
    let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(0);
    let screen = engine.current_screen();
    assert!(caption_content(&screen, "pending_updates_caption").is_none());
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
fn test_my_info_emits_both_captions_in_order_pending_then_sync() {
    let now = 1_700_000_000u64;
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_pending_updates(2)
        .with_last_sync_seconds(Some(now - 120))
        .with_now_seconds(now);
    let screen = engine.current_screen();
    let positions: Vec<usize> = screen
        .components
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c {
            Component::Text { id, .. }
                if id == "pending_updates_caption" || id == "last_sync_caption" =>
            {
                Some(i)
            }
            _ => None,
        })
        .collect();
    assert_eq!(positions.len(), 2, "expected both captions in the screen");
    assert!(
        positions[0] < positions[1],
        "pending_updates_caption must appear before last_sync_caption"
    );
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
}

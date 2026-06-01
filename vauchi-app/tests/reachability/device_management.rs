// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeviceManagementEngine`.
//!
//! Single `screen_id` (`device_management`) engine. The base screen
//! lists devices and offers `link_device` / `revoke_device`. Tapping
//! a revocable device (`ListItemSelected` on `device_list`) sets
//! `pending_revoke_index` and adds an `InlineConfirm` — but the
//! resulting screen keeps `screen_id == "device_management"`, so the
//! BFS `screen_id` dedup
//! (`screen_walker::all_reachable_screens`) collapses it into the
//! base. This test therefore pins the base screen's affordance set,
//! exactly as `recovery.rs` does for its shared-id steps.
//!
//! The dynamic `confirm_revoke_device:{i}` / `cancel_revoke_device:{i}`
//! ids the `InlineConfirm` emits (and the irrevocable revoke they
//! drive — ADR-022) are covered by the inline tests in
//! `core/vauchi-app/src/ui/device_management.rs`
//! (`confirmed_revoke_index` / cancel paths). Declaring them here
//! would make them orphan handlers (their screen is never recorded
//! by BFS), so they are deliberately excluded.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{DeviceListItem, DeviceManagementEngine, WorkflowEngine};

/// Action ids the base `device_management` screen emits and
/// `DeviceManagementEngine::handle_action` consumes —
/// `core/vauchi-app/src/ui/device_management.rs`.
const HANDLED: &[&str] = &["link_device", "revoke_device"];

fn engine() -> DeviceManagementEngine {
    // Current device + one revocable + one already-revoked: the
    // realistic post-link state, and the `can_revoke` predicate is
    // satisfied so `revoke_device` renders enabled.
    DeviceManagementEngine::new(vec![
        DeviceListItem {
            device_index: 0,
            device_name: "iPhone".into(),
            public_key_prefix: "a1b2c3d4".into(),
            is_current: true,
            is_active: true,
        },
        DeviceListItem {
            device_index: 1,
            device_name: "Desktop".into(),
            public_key_prefix: "e5f6a7b8".into(),
            is_current: false,
            is_active: true,
        },
        DeviceListItem {
            device_index: 2,
            device_name: "Old Phone".into(),
            public_key_prefix: "c9d0e1f2".into(),
            is_current: false,
            is_active: false,
        },
    ])
}

// @internal
#[test]
fn device_management_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "device_management");
    assert_reachability(&engine, HANDLED);
}

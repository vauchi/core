// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! More menu engine — navigation hub for secondary screens.
//!
//! Displays a single `ActionList` with entries for screens that moved
//! out of the top-level tab bar: Sync, Devices, Settings, Backup,
//! Privacy, Help.  Each entry emits `OpenContact { contact_id }` where
//! the contact_id is a screen ID string — `route_result` in `routing.rs`
//! maps it to the target `AppScreen`.

use crate::ui::*;

/// Navigation targets exposed through the More menu.
const MORE_ITEMS: &[(&str, &str)] = &[
    ("sync", "Sync"),
    ("device_linking", "Devices"),
    ("settings", "Settings"),
    ("backup", "Backup"),
    ("privacy", "Privacy"),
    ("help", "Help"),
];

/// Engine that renders a list of navigation targets for the "More" tab.
#[derive(Clone, Debug)]
pub struct MoreEngine;

impl Default for MoreEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MoreEngine {
    pub fn new() -> Self {
        Self
    }
}

impl WorkflowEngine for MoreEngine {
    fn current_screen(&self) -> ScreenModel {
        let items: Vec<ActionListItem> = MORE_ITEMS
            .iter()
            .map(|(id, label)| ActionListItem {
                id: (*id).into(),
                label: (*label).into(),
                icon: None,
                detail: None,
            })
            .collect();

        ScreenModel {
            screen_id: "more".into(),
            title: "More".into(),
            subtitle: None,
            components: vec![Component::ActionList {
                id: "more_menu".into(),
                items,
            }],
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id }
            | UserAction::ListItemSelected {
                item_id: action_id, ..
            } => {
                // Reuse OpenContact pattern — routing.rs maps the ID to an AppScreen.
                ActionResult::OpenContact {
                    contact_id: action_id,
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

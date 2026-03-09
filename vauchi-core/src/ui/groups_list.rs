// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups engine — displays and manages contact groups.

use crate::ui::*;

/// Engine that displays contact groups.
#[derive(Clone, Debug)]
pub struct GroupsEngine {
    groups: Vec<ContactItem>,
}

impl GroupsEngine {
    pub fn new(groups: Vec<ContactItem>) -> Self {
        Self { groups }
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "groups_list".into(),
            title: "Contact Groups".into(),
            subtitle: None,
            components: vec![Component::ContactList {
                id: "groups".into(),
                contacts: self.groups.clone(),
                searchable: true,
            }],
            actions: vec![ScreenAction {
                id: "create_group".into(),
                label: "Create Group".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: None,
        }
    }
}

impl WorkflowEngine for GroupsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "create_group" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

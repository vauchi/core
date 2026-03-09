// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group detail engine — shows details of a single contact group.

use crate::ui::*;

/// Engine that displays details of a single contact group.
#[derive(Clone, Debug)]
pub struct GroupDetailEngine {
    group_id: String,
    group_name: String,
    members: Vec<ContactItem>,
}

impl GroupDetailEngine {
    pub fn new(group_id: String, group_name: String, members: Vec<ContactItem>) -> Self {
        Self {
            group_id,
            group_name,
            members,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "group_detail".into(),
            title: self.group_name.clone(),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "group_info".into(),
                    icon: Some("group".into()),
                    title: "Group Info".into(),
                    items: vec![InfoItem {
                        icon: Some("members".into()),
                        title: "Members".into(),
                        detail: format!("{}", self.members.len()),
                    }],
                },
                Component::ContactList {
                    id: "members".into(),
                    contacts: self.members.clone(),
                    searchable: false,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "rename".into(),
                    label: "Rename".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "delete_group".into(),
                    label: "Delete Group".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }
}

impl WorkflowEngine for GroupDetailEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "rename" | "delete_group" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

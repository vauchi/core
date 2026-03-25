// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group detail engine — shows details of a single contact group.

use crate::ui::*;

/// Engine that displays details of a single contact group.
#[derive(Clone, Debug)]
pub struct GroupDetailEngine {
    #[allow(dead_code)] // Used by future save logic (rename/delete group)
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
                        accessible_label: None,
                        accessible_hint: None,
                    }],
                    accessible_label: None,
                    accessible_hint: None,
                },
                Component::ContactList {
                    id: "members".into(),
                    contacts: self.members.clone(),
                    searchable: false,
                    accessible_label: None,
                    accessible_hint: None,
                },
            ],
            actions: {
                let mut actions: Vec<ScreenAction> = self
                    .members
                    .iter()
                    .map(|m| ScreenAction {
                        id: format!("preview-as-member:{}", m.id),
                        label: format!("Preview as {}", m.name),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    })
                    .collect();
                actions.push(ScreenAction {
                    id: "rename".into(),
                    label: "Rename".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                });
                actions.push(ScreenAction {
                    id: "delete_group".into(),
                    label: "Delete Group".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                });
                actions
            },
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
            UserAction::ActionPressed { action_id } => {
                if let Some(contact_id) = action_id.strip_prefix("preview-as-member:") {
                    return ActionResult::PreviewAs {
                        contact_id: contact_id.to_string(),
                    };
                }
                match action_id.as_str() {
                    "rename" => ActionResult::ShowAlert {
                        title: "Coming Soon".into(),
                        message: "Group renaming will be available in a future update.".into(),
                    },
                    "delete_group" => ActionResult::ShowAlert {
                        title: "Coming Soon".into(),
                        message: "Group deletion will be available in a future update.".into(),
                    },
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                }
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

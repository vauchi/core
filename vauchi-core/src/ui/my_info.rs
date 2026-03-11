// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! MyInfo screen engine — shows user's own card, entries, and visibility controls.

use crate::ui::*;

/// Progress summary for the MyInfo screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MyInfoProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
}

/// MyInfo screen engine — shows user's own card, entries, and visibility.
#[derive(Clone, Debug)]
pub struct MyInfoEngine {
    contacts: Vec<ContactItem>,
    progress: MyInfoProgress,
}

impl MyInfoEngine {
    pub fn new(contacts: Vec<ContactItem>, progress: MyInfoProgress) -> Self {
        Self { contacts, progress }
    }
}

impl WorkflowEngine for MyInfoEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Show setup progress if not complete
        if self.progress.completed_steps < self.progress.total_steps {
            components.push(Component::StatusIndicator {
                id: "setup_progress".into(),
                icon: Some("setup".into()),
                title: "Complete your setup".into(),
                detail: Some(format!(
                    "{} of {} steps done",
                    self.progress.completed_steps, self.progress.total_steps
                )),
                status: Status::InProgress,
            });
        }

        // Contact list (show up to 5 recent)
        let recent: Vec<ContactItem> = self.contacts.iter().take(5).cloned().collect();
        components.push(Component::ContactList {
            id: "recent_contacts".into(),
            contacts: recent,
            searchable: false,
        });

        let mut actions = vec![ScreenAction {
            id: "add_field".into(),
            label: "Add Entry".into(),
            style: ActionStyle::Primary,
            enabled: true,
        }];
        if !self.contacts.is_empty() {
            actions.push(ScreenAction {
                id: "view_all".into(),
                label: "View All Contacts".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            });
        }

        ScreenModel {
            screen_id: "my_info".into(),
            title: "My Info".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemSelected { item_id, .. } => ActionResult::OpenContact {
                contact_id: item_id,
            },
            UserAction::ActionPressed { action_id } if action_id == "add_field" => {
                ActionResult::NavigateTo(self.current_screen()) // caller handles navigation
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

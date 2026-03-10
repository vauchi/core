// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Home screen engine — single screen showing recent contacts and setup progress.

use crate::ui::*;

/// Progress summary for the home screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HomeProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
}

/// Home screen engine — single screen showing recent contacts and setup progress.
#[derive(Clone, Debug)]
pub struct HomeEngine {
    contacts: Vec<ContactItem>,
    progress: HomeProgress,
}

impl HomeEngine {
    pub fn new(contacts: Vec<ContactItem>, progress: HomeProgress) -> Self {
        Self { contacts, progress }
    }
}

impl WorkflowEngine for HomeEngine {
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
            screen_id: "home".into(),
            title: "Home".into(),
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

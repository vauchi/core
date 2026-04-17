// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Archived contacts engine — view and unarchive previously archived contacts.

use crate::ui::*;

/// Engine that displays archived contacts and allows unarchiving them.
#[derive(Clone, Debug)]
pub struct ArchivedContactsEngine {
    /// (contact_id, display_name) pairs for archived contacts.
    contacts: Vec<(String, String)>,
}

impl ArchivedContactsEngine {
    pub fn new(contacts: Vec<(String, String)>) -> Self {
        Self { contacts }
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.contacts.is_empty() {
            vec![Component::Text {
                id: "no_archived".into(),
                content: "No archived contacts.".into(),
                style: TextStyle::Body,
            }]
        } else {
            vec![Component::ActionList {
                id: "archived_contacts".into(),
                items: self
                    .contacts
                    .iter()
                    .map(|(id, name)| ActionListItem {
                        id: format!("unarchive_{id}"),
                        label: name.clone(),
                        icon: None,
                        detail: Some("Tap to unarchive".into()),
                        a11y: None,
                        info_key: None,
                    })
                    .collect(),
            }]
        };

        ScreenModel {
            screen_id: "archived_contacts".into(),
            title: "Archived Contacts".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for ArchivedContactsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id.starts_with("unarchive_") => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

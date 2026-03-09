// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact list engine — searchable list of all contacts.

use crate::ui::*;

/// Contact list engine — full contact list with search filtering.
#[derive(Clone, Debug)]
pub struct ContactListEngine {
    all_contacts: Vec<ContactItem>,
    search_query: String,
}

impl ContactListEngine {
    pub fn new(contacts: Vec<ContactItem>) -> Self {
        Self {
            all_contacts: contacts,
            search_query: String::new(),
        }
    }

    fn filtered_contacts(&self) -> Vec<ContactItem> {
        if self.search_query.is_empty() {
            // Clone is required: ScreenModel owns its components, so ContactList
            // needs an owned Vec<ContactItem>. Caching would add complexity for
            // a list that is small in practice (< 1000 contacts).
            return self.all_contacts.clone();
        }
        let query_lower = self.search_query.to_lowercase();
        self.all_contacts
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect()
    }
}

impl WorkflowEngine for ContactListEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_list".into(),
            title: "Contacts".into(),
            subtitle: None,
            components: vec![Component::ContactList {
                id: "contacts".into(),
                contacts: self.filtered_contacts(),
                searchable: true,
            }],
            actions: vec![ScreenAction {
                id: "add_contact".into(),
                label: "Add Contact".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::SearchChanged { query, .. } => {
                self.search_query = query;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { item_id, .. } => ActionResult::OpenContact {
                contact_id: item_id,
            },
            UserAction::ActionPressed { action_id } if action_id == "add_contact" => {
                ActionResult::NavigateTo(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

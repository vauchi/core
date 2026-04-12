// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact list engine — searchable list of all contacts with group filtering.

use std::collections::HashMap;

use crate::ui::*;

/// Contact list engine — full contact list with search and group filtering.
#[derive(Clone, Debug)]
pub struct ContactListEngine {
    all_contacts: Vec<ContactItem>,
    search_query: String,
    /// Active group filter: None = show all, Some(group_id) = show only members.
    group_filter: Option<String>,
    /// Available groups: (group_id, group_name).
    available_groups: Vec<(String, String)>,
    /// Group memberships: group_id -> list of contact_ids.
    group_memberships: HashMap<String, Vec<String>>,
}

impl ContactListEngine {
    pub fn new(contacts: Vec<ContactItem>) -> Self {
        Self {
            all_contacts: contacts,
            search_query: String::new(),
            group_filter: None,
            available_groups: Vec::new(),
            group_memberships: HashMap::new(),
        }
    }

    /// Create engine with group information for filtering.
    pub fn with_groups(
        contacts: Vec<ContactItem>,
        groups: Vec<(String, String)>,
        memberships: HashMap<String, Vec<String>>,
    ) -> Self {
        Self {
            all_contacts: contacts,
            search_query: String::new(),
            group_filter: None,
            available_groups: groups,
            group_memberships: memberships,
        }
    }

    fn filtered_contacts(&self) -> Vec<ContactItem> {
        // Clone is required: ScreenModel owns its components, so ContactList
        // needs an owned Vec<ContactItem>. Caching would add complexity for
        // a list that is small in practice (< 1000 contacts).
        let base = if let Some(ref group_id) = self.group_filter {
            let member_ids = self.group_memberships.get(group_id);
            self.all_contacts
                .iter()
                .filter(|c| member_ids.map(|ids| ids.contains(&c.id)).unwrap_or(false))
                .cloned()
                .collect()
        } else {
            self.all_contacts.clone()
        };

        if self.search_query.is_empty() {
            return base;
        }

        let query_lower = self.search_query.to_lowercase();
        base.into_iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&query_lower)
                    || c.searchable_fields
                        .iter()
                        .any(|f| f.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
}

impl WorkflowEngine for ContactListEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut actions = vec![ScreenAction {
            id: "add_contact".into(),
            label: "Add Contact".into(),
            style: ActionStyle::Primary,
            enabled: true,
        }];

        // Add group filter actions
        for (gid, gname) in &self.available_groups {
            actions.push(ScreenAction {
                id: format!("filter_group:{gid}"),
                label: gname.clone(),
                style: if self.group_filter.as_deref() == Some(gid) {
                    ActionStyle::Primary
                } else {
                    ActionStyle::Secondary
                },
                enabled: true,
            });
        }

        // Add clear filter action when a filter is active
        if self.group_filter.is_some() {
            actions.push(ScreenAction {
                id: "filter_group_clear".into(),
                label: "All Contacts".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            });
        }

        // Empty state: when the user has no contacts at all (not just
        // empty search results), show guidance encouraging first exchange.
        let filtered = self.filtered_contacts();
        let components = if self.all_contacts.is_empty() {
            vec![Component::InfoPanel {
                id: "empty_state".into(),
                icon: Some("people".into()),
                title: "No contacts yet".into(),
                items: vec![InfoItem {
                    icon: Some("exchange".into()),
                    title: "Exchange cards in person".into(),
                    detail: "Meet someone nearby and share your contact card securely.".into(),
                }],
                a11y: None,
            }]
        } else {
            vec![Component::ContactList {
                id: "contacts".into(),
                contacts: filtered,
                searchable: true,
            }]
        };

        // Add exchange shortcut when empty
        if self.all_contacts.is_empty() {
            actions.insert(
                0,
                ScreenAction {
                    id: "go_exchange".into(),
                    label: "Exchange Now".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            );
        }

        ScreenModel {
            screen_id: "contact_list".into(),
            title: "Contacts".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
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
            UserAction::ActionPressed { ref action_id } if action_id == "add_contact" => {
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ActionPressed { ref action_id }
                if action_id.starts_with("filter_group:") =>
            {
                let group_id = action_id.strip_prefix("filter_group:").unwrap().to_string();
                self.group_filter = Some(group_id);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { ref action_id } if action_id == "filter_group_clear" => {
                self.group_filter = None;
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

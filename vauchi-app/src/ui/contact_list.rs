// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact list engine — searchable list of all contacts with group filtering.

use std::collections::HashMap;

use crate::ui::*;

/// Contact list engine — full contact list with search and group filtering.
#[derive(Clone, Debug)]
pub struct ContactListEngine {
    all_contacts: Vec<Item>,
    search_query: String,
    /// Active group filter: None = show all, Some(group_id) = show only members.
    group_filter: Option<String>,
    /// Available groups: (group_id, group_name).
    available_groups: Vec<(String, String)>,
    /// Group memberships: group_id -> list of contact_ids.
    group_memberships: HashMap<String, Vec<String>>,
}

impl ContactListEngine {
    pub fn new(contacts: Vec<Item>) -> Self {
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
        contacts: Vec<Item>,
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

    fn filtered_contacts(&self) -> Vec<Item> {
        // Clone is required: ScreenModel owns its components, so ContactList
        // needs an owned Vec<Item>. Caching would add complexity for
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
            a11y: None,
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
                a11y: None,
            });
        }

        // Add clear filter action when a filter is active
        if self.group_filter.is_some() {
            actions.push(ScreenAction {
                id: "filter_group_clear".into(),
                label: "All Contacts".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
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

        // Archived contacts link
        actions.push(ScreenAction {
            id: "view_archived".into(),
            label: "Archived Contacts".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        // Find duplicates action
        actions.push(ScreenAction {
            id: "find_duplicates".into(),
            label: "Find Duplicates".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        // Add exchange shortcut when empty
        if self.all_contacts.is_empty() {
            actions.insert(
                0,
                ScreenAction {
                    id: "go_exchange".into(),
                    label: "Exchange Now".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
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
            // `add_contact` is intercepted by `AppEngine` and routed to
            // the Exchange screen (see `app_engine/mod.rs`). If the engine
            // is driven standalone (e.g. in a unit test) the action falls
            // through to the `_` arm and produces a harmless screen
            // refresh — the engine itself has no navigation authority.
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
            UserAction::ListItemAction {
                item_id, action_id, ..
            } => {
                // Validate that `action_id` was one we actually offered
                // for this item. Stale/forged action ids drop to a
                // no-op screen refresh rather than performing a random
                // mutation on the wrong contact.
                let offered = self
                    .all_contacts
                    .iter()
                    .find(|c| c.id == item_id)
                    .and_then(|c| c.actions.iter().find(|a| a.id == action_id))
                    .map(|a| a.kind);
                if let Some(kind) = offered
                    && let Some(contact_kind) = contact_action_kind_from(kind)
                {
                    return ActionResult::ContactAction {
                        contact_id: item_id,
                        kind: contact_kind,
                    };
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

fn contact_action_kind_from(kind: ListItemActionKind) -> Option<ContactActionKind> {
    match kind {
        ListItemActionKind::Archive => Some(ContactActionKind::Archive),
        ListItemActionKind::Unarchive => Some(ContactActionKind::Unarchive),
        ListItemActionKind::Hide => Some(ContactActionKind::Hide),
        ListItemActionKind::Unhide => Some(ContactActionKind::Unhide),
        ListItemActionKind::Delete => Some(ContactActionKind::Delete),
        ListItemActionKind::Undelete => Some(ContactActionKind::Undelete),
        ListItemActionKind::Custom => None,
    }
}

// INLINE_TEST_REQUIRED: tests exercise the private `contact_action_kind_from`
// helper. Moving them to tests/ would require making the helper public, which
// is a wider surface than the invariant they protect.
#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, actions: Vec<ListItemAction>) -> Item {
        Item {
            id: id.into(),
            name: format!("Contact {id}"),
            subtitle: None,
            avatar_initials: "C".into(),
            status: None,
            searchable_fields: vec![],
            actions,
            a11y: None,
        }
    }

    fn archive_action() -> ListItemAction {
        ListItemAction {
            id: "archive".into(),
            label: "Archive".into(),
            kind: ListItemActionKind::Archive,
            destructive: false,
        }
    }

    // @internal
    #[test]
    fn list_item_action_archive_returns_contact_action_archive() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "archive".into(),
        });
        assert_eq!(
            result,
            ActionResult::ContactAction {
                contact_id: "c1".into(),
                kind: ContactActionKind::Archive,
            }
        );
    }

    // @internal
    #[test]
    fn list_item_action_unknown_action_id_is_noop() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "hide".into(), // not in item's actions
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn list_item_action_unknown_item_id_is_noop() {
        let mut engine = ContactListEngine::new(vec![item("c1", vec![archive_action()])]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c999".into(),
            action_id: "archive".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn list_item_action_custom_kind_is_noop() {
        let mut engine = ContactListEngine::new(vec![item(
            "c1",
            vec![ListItemAction {
                id: "pin".into(),
                label: "Pin".into(),
                kind: ListItemActionKind::Custom,
                destructive: false,
            }],
        )]);
        let result = engine.handle_action(UserAction::ListItemAction {
            component_id: "contacts".into(),
            item_id: "c1".into(),
            action_id: "pin".into(),
        });
        // Custom kinds require dedicated handling — engine refuses rather
        // than guessing, so screen refreshes without a mutation.
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn all_kinds_round_trip() {
        // Every ListItemActionKind (except Custom) must map to a
        // ContactActionKind — otherwise the engine silently drops it.
        use ListItemActionKind::*;
        for (k, expected) in [
            (Archive, Some(ContactActionKind::Archive)),
            (Unarchive, Some(ContactActionKind::Unarchive)),
            (Hide, Some(ContactActionKind::Hide)),
            (Unhide, Some(ContactActionKind::Unhide)),
            (Delete, Some(ContactActionKind::Delete)),
            (Undelete, Some(ContactActionKind::Undelete)),
            (Custom, None),
        ] {
            assert_eq!(contact_action_kind_from(k), expected, "kind={k:?}");
        }
    }
}

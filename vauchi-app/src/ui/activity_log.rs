// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Activity log engine — shows recent activity entries for contacts.

use crate::notification_types::ActivityLogEntry;
use crate::ui::*;

/// A displayable activity log entry with resolved contact name.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActivityLogItem {
    pub event_key: String,
    pub entry: ActivityLogEntry,
    pub contact_name: String,
    pub created_at: u64,
}

/// Engine that displays the activity log screen.
#[derive(Clone, Debug)]
pub struct ActivityLogEngine {
    items: Vec<ActivityLogItem>,
}

impl ActivityLogEngine {
    pub fn new(items: Vec<ActivityLogItem>) -> Self {
        Self { items }
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.items.is_empty() {
            vec![Component::Text {
                id: "empty_state".into(),
                content: "No recent activity. Updates from your contacts and delivery status will appear here.".into(),
                style: TextStyle::Body,
            }]
        } else {
            let list_items: Vec<ActionListItem> = self
                .items
                .iter()
                .map(|item| {
                    let (label, detail) = format_entry(&item.entry, &item.contact_name);
                    ActionListItem {
                        id: item.event_key.clone(),
                        label,
                        icon: None,
                        detail,
                        a11y: None,
                        info_key: None,
                    }
                })
                .collect();

            vec![Component::ActionList {
                id: "activity_list".into(),
                items: list_items,
            }]
        };

        ScreenModel {
            screen_id: "activity_log".into(),
            title: "Activity".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

/// Format an `ActivityLogEntry` into a (label, detail) pair for display.
fn format_entry(entry: &ActivityLogEntry, name: &str) -> (String, Option<String>) {
    match entry {
        ActivityLogEntry::CardUpdateReceived { changed_fields, .. } => {
            let detail = if changed_fields.is_empty() {
                None
            } else {
                Some(changed_fields.join(", "))
            };
            (format!("{name} updated their card"), detail)
        }
        ActivityLogEntry::CardUpdateDelivered { .. } => (format!("Card delivered to {name}"), None),
        ActivityLogEntry::CardUpdatePending { .. } => (format!("Card pending: {name}"), None),
        ActivityLogEntry::CardUpdateFailed { reason, .. } => {
            (format!("Card failed: {name}"), Some(reason.clone()))
        }
        ActivityLogEntry::ContactAdded { .. } => (format!("New contact: {name}"), None),
        ActivityLogEntry::EmergencyAlertReceived { .. } => {
            (format!("Emergency alert from {name}"), None)
        }
        ActivityLogEntry::DuressAlertReceived { .. } => (format!("Duress alert from {name}"), None),
        ActivityLogEntry::OwnCardUpdated { changed_fields } => {
            let detail = if changed_fields.is_empty() {
                None
            } else {
                Some(changed_fields.join(", "))
            };
            ("You updated your card".to_string(), detail)
        }
        ActivityLogEntry::ContactRemoved { .. } => (format!("Removed contact: {name}"), None),
    }
}

impl WorkflowEngine for ActivityLogEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemSelected { item_id, .. } => {
                if let Some(item) = self.items.iter().find(|i| i.event_key == item_id) {
                    // OwnCardUpdated and ContactRemoved have no meaningful
                    // navigation target (own card is not a contact; removed
                    // contacts no longer exist).
                    if matches!(
                        item.entry,
                        ActivityLogEntry::OwnCardUpdated { .. }
                            | ActivityLogEntry::ContactRemoved { .. }
                    ) {
                        return ActionResult::UpdateScreen(self.build_screen());
                    }
                    let contact_id = item.entry.contact_id().to_string();
                    return ActionResult::OpenContact { contact_id };
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

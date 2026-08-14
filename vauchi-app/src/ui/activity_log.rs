// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Activity log engine — shows recent activity entries for contacts.

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    locale: Locale,
}

impl ActivityLogEngine {
    pub fn new(items: Vec<ActivityLogItem>) -> Self {
        Self {
            items,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.items.is_empty() {
            vec![Component::Text {
                a11y: None,
                id: "empty_state".into(),
                content: get_string(self.locale, "activity_log.empty_state"),
                style: TextStyle::Body,
            }]
        } else {
            let list_items: Vec<ActionListItem> = self
                .items
                .iter()
                .map(|item| {
                    let (label, detail) =
                        format_entry(&item.entry, &item.contact_name, self.locale);
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
            title: get_string(self.locale, "activity_log.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

/// Format an `ActivityLogEntry` into a (label, detail) pair for display.
fn format_entry(entry: &ActivityLogEntry, name: &str, locale: Locale) -> (String, Option<String>) {
    match entry {
        ActivityLogEntry::CardUpdateReceived { changed_fields, .. } => {
            let detail = if changed_fields.is_empty() {
                None
            } else {
                Some(changed_fields.join(", "))
            };
            (
                get_string_with_args(
                    locale,
                    "activity_log.card_update_received",
                    &[("name", name)],
                ),
                detail,
            )
        }
        ActivityLogEntry::CardUpdateDelivered { .. } => (
            get_string_with_args(locale, "activity_log.card_delivered", &[("name", name)]),
            None,
        ),
        ActivityLogEntry::CardUpdatePending { .. } => (
            get_string_with_args(locale, "activity_log.card_pending", &[("name", name)]),
            None,
        ),
        ActivityLogEntry::CardUpdateFailed { reason, .. } => (
            get_string_with_args(locale, "activity_log.card_failed", &[("name", name)]),
            Some(reason.clone()),
        ),
        ActivityLogEntry::ContactAdded { .. } => (
            get_string_with_args(locale, "activity_log.new_contact", &[("name", name)]),
            None,
        ),
        ActivityLogEntry::EmergencyAlertReceived { .. } => (
            get_string_with_args(locale, "activity_log.emergency_alert", &[("name", name)]),
            None,
        ),
        ActivityLogEntry::DuressAlertReceived { .. } => (
            get_string_with_args(locale, "activity_log.duress_alert", &[("name", name)]),
            None,
        ),
        ActivityLogEntry::OwnCardUpdated { changed_fields } => {
            let detail = if changed_fields.is_empty() {
                None
            } else {
                Some(changed_fields.join(", "))
            };
            (get_string(locale, "activity_log.own_card_updated"), detail)
        }
        ActivityLogEntry::ContactRemoved { .. } => (
            get_string_with_args(locale, "activity_log.contact_removed", &[("name", name)]),
            None,
        ),
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

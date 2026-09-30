// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Update Delivery engine — where a card update has reached, one list at a
//! time behind a Recent / Pending / Failed switch (#445).
//!
//! The switch opens on the most urgent non-empty list (Failed, then
//! Pending, then Recent), so a failure is never hidden behind a tab (owner
//! decision 2026-09-30). Row details arrive localized from the loader.
//!
//! See `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`
//! for the architectural context (Pair 1 — DeliveryStatus retirement).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Action id of the "Retry all failed" button.
pub const RETRY_ALL_ACTION_ID: &str = "retry_all";

const FILTER_ID: &str = "delivery_filter";
const ROWS_ID: &str = "deliveries";
const ACTIONS_ID: &str = "delivery_actions";
const HELP_ID: &str = "delivery_help";

/// A single delivery item with status and retry info.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeliveryItem {
    pub message_id: String,
    pub contact_id: String,
    pub contact_name: String,
    pub status: Status,
    pub detail: Option<String>,
    pub retryable: bool,
}

/// A pending retry entry — distinct from `DeliveryItem` because retries
/// are scheduled separately from the original delivery record.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RetryEntry {
    pub message_id: String,
    pub contact_id: String,
    pub contact_name: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub max_exceeded: bool,
    /// Seconds until the next attempt; `None` when unknown.
    #[serde(default)]
    pub next_retry_in_secs: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Recent,
    Pending,
    Failed,
}

impl Tab {
    fn id(self) -> &'static str {
        match self {
            Tab::Recent => "recent",
            Tab::Pending => "pending",
            Tab::Failed => "failed",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "recent" => Some(Tab::Recent),
            "pending" => Some(Tab::Pending),
            "failed" => Some(Tab::Failed),
            _ => None,
        }
    }
}

/// One row of the screen, whichever record it came from.
struct Row {
    message_id: String,
    contact_id: String,
    contact_name: String,
    detail: Option<String>,
    tab: Tab,
    retryable: bool,
}

/// Engine that displays delivery status for a set of contacts.
#[derive(Clone, Debug)]
pub struct DeliveryStatusEngine {
    items: Vec<DeliveryItem>,
    retries: Vec<RetryEntry>,
    locale: Locale,
    chosen: Option<Tab>,
}

impl DeliveryStatusEngine {
    pub fn new(items: Vec<DeliveryItem>) -> Self {
        Self {
            items,
            retries: Vec::new(),
            locale: Locale::English,
            chosen: None,
        }
    }

    pub fn with_retries(mut self, retries: Vec<RetryEntry>) -> Self {
        self.retries = retries;
        self
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-13).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn rows(&self) -> Vec<Row> {
        let items = self.items.iter().map(|item| Row {
            message_id: item.message_id.clone(),
            contact_id: item.contact_id.clone(),
            contact_name: item.contact_name.clone(),
            detail: item.detail.clone(),
            tab: match item.status {
                Status::Failed | Status::Warning => Tab::Failed,
                Status::Pending | Status::InProgress => Tab::Pending,
                _ => Tab::Recent,
            },
            retryable: item.retryable,
        });
        let retries = self.retries.iter().map(|retry| Row {
            message_id: retry.message_id.clone(),
            contact_id: retry.contact_id.clone(),
            contact_name: retry.contact_name.clone(),
            detail: Some(self.retry_detail(retry)),
            tab: if retry.max_exceeded {
                Tab::Failed
            } else {
                Tab::Pending
            },
            retryable: false,
        });
        items.chain(retries).collect()
    }

    fn retry_detail(&self, retry: &RetryEntry) -> String {
        let attempt = retry.attempt.to_string();
        let max = retry.max_attempts.to_string();
        if retry.max_exceeded {
            return get_string_with_args(
                self.locale,
                "delivery_status.max_attempts_exceeded",
                &[("max", &max)],
            );
        }
        match retry.next_retry_in_secs {
            Some(secs) if secs > 0 => get_string_with_args(
                self.locale,
                "delivery_status.retry_next_minutes",
                &[
                    ("attempt", &attempt),
                    ("max", &max),
                    ("minutes", &secs.div_ceil(60).to_string()),
                ],
            ),
            Some(_) => get_string_with_args(
                self.locale,
                "delivery_status.retry_now",
                &[("attempt", &attempt), ("max", &max)],
            ),
            None => get_string_with_args(
                self.locale,
                "delivery_status.attempt_of",
                &[("attempt", &attempt), ("max", &max)],
            ),
        }
    }

    /// The chosen list, else the most urgent non-empty one.
    fn tab(&self, rows: &[Row]) -> Tab {
        self.chosen.unwrap_or_else(|| {
            [Tab::Failed, Tab::Pending]
                .into_iter()
                .find(|tab| rows.iter().any(|row| row.tab == *tab))
                .unwrap_or(Tab::Recent)
        })
    }

    fn build_screen(&self) -> ScreenModel {
        let rows = self.rows();
        let mut components = Vec::new();
        if rows.is_empty() {
            components.push(Component::InfoPanel {
                id: "empty".into(),
                icon: Some("checkmark".into()),
                title: self.t("delivery_status.all_delivered"),
                items: vec![],
                a11y: None,
            });
        } else {
            let tab = self.tab(&rows);
            components.push(self.switch(&rows, tab));
            components.push(self.list_or_empty(&rows, tab));
            if tab == Tab::Failed && rows.iter().any(|r| r.tab == Tab::Failed && r.retryable) {
                components.push(Component::ButtonList {
                    id: ACTIONS_ID.into(),
                    items: vec![ActionListItem {
                        id: RETRY_ALL_ACTION_ID.into(),
                        label: self.t("delivery_status.retry_failed_button"),
                        icon: None,
                        detail: None,
                        a11y: None,
                        info_key: None,
                    }],
                });
            }
        }
        components.push(Component::SettingsGroup {
            id: HELP_ID.into(),
            label: self.t("settings.help"),
            items: vec![SettingsItem {
                id: "help".into(),
                label: self.t("delivery_status.help_label"),
                kind: SettingsItemKind::Link { detail: None },
                subtitle: None,
                a11y: None,
                info_key: None,
            }],
        });

        ScreenModel {
            screen_id: "delivery_status".into(),
            title: self.t("delivery_status.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn switch(&self, rows: &[Row], tab: Tab) -> Component {
        let option = |t: Tab, key: &str| DropdownOption {
            id: t.id().into(),
            label: get_string_with_args(
                self.locale,
                key,
                &[(
                    "count",
                    &rows.iter().filter(|r| r.tab == t).count().to_string(),
                )],
            ),
        };
        Component::Dropdown {
            id: FILTER_ID.into(),
            label: self.t("delivery_status.filter_label"),
            selected: Some(tab.id().into()),
            options: vec![
                option(Tab::Recent, "delivery_status.tab_recent"),
                option(Tab::Pending, "delivery_status.tab_pending"),
                option(Tab::Failed, "delivery_status.tab_failed"),
            ],
            a11y: None,
        }
    }

    fn list_or_empty(&self, rows: &[Row], tab: Tab) -> Component {
        let items: Vec<Item> = rows
            .iter()
            .filter(|row| row.tab == tab)
            .map(|row| Item {
                id: row.message_id.clone(),
                name: row.contact_name.clone(),
                subtitle: row.detail.clone(),
                initials: initials(&row.contact_name),
                status: None,
                actions: vec![],
                a11y: None,
            })
            .collect();
        if items.is_empty() {
            let key = match tab {
                Tab::Recent => "delivery_status.empty_recent",
                Tab::Pending => "delivery_status.empty_pending",
                Tab::Failed => "delivery_status.empty_failed",
            };
            return Component::Text {
                id: "empty_list".into(),
                content: self.t(key),
                style: TextStyle::Body,
                a11y: None,
            };
        }
        Component::List {
            id: ROWS_ID.into(),
            items,
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        }
    }

    fn retry_all(&self) -> ActionResult {
        let message_ids: Vec<String> = self
            .items
            .iter()
            .filter(|i| i.retryable)
            .map(|i| i.message_id.clone())
            .collect();
        if message_ids.is_empty() {
            ActionResult::UpdateScreen(self.build_screen())
        } else {
            ActionResult::RetryFailedDeliveries { message_ids }
        }
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

impl WorkflowEngine for DeliveryStatusEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } => match component_id.as_str() {
                FILTER_ID => {
                    if let Some(tab) = Tab::from_id(&item_id) {
                        self.chosen = Some(tab);
                    }
                    ActionResult::UpdateScreen(self.build_screen())
                }
                ROWS_ID => match self.rows().into_iter().find(|r| r.message_id == item_id) {
                    Some(row) => ActionResult::OpenContact {
                        contact_id: row.contact_id,
                    },
                    None => ActionResult::UpdateScreen(self.build_screen()),
                },
                ACTIONS_ID if item_id == RETRY_ALL_ACTION_ID => self.retry_all(),
                HELP_ID => ActionResult::ShowInfoOverlay {
                    title: self.t("delivery_status.help_label"),
                    body: self.t("delivery_status.help_body"),
                },
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: none of the rendering is private any more; the
// behaviour lives in vauchi-app/tests/it/delivery_screen_tests.rs. This
// keeps the adversarial input check next to the handler it guards.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn adversarial_ids_do_not_panic() {
        let mut engine = DeliveryStatusEngine::new(vec![]);
        for case in &[
            "",
            "retry:",
            "retry::::",
            "retry:🦀",
            "retry:'; DROP TABLE--",
        ] {
            let pressed = engine.handle_action(UserAction::ActionPressed {
                action_id: (*case).into(),
            });
            assert!(matches!(pressed, ActionResult::UpdateScreen(_)));
            for component in [FILTER_ID, ROWS_ID, ACTIONS_ID, "unknown"] {
                let selected = engine.handle_action(UserAction::ListItemSelected {
                    component_id: component.into(),
                    item_id: (*case).into(),
                });
                assert!(
                    matches!(selected, ActionResult::UpdateScreen(_)),
                    "{component} {case}"
                );
            }
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery status engine — shows delivery state per contact in three
//! sections (Recent, Failed, Pending Retries).
//!
//! See `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`
//! for the architectural context (Pair 1 — DeliveryStatus retirement).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Action id emitted by the "retry all failed" footer button.
pub const RETRY_ALL_ACTION_ID: &str = "retry_all";

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
}

/// Engine that displays delivery status for a set of contacts.
#[derive(Clone, Debug)]
pub struct DeliveryStatusEngine {
    items: Vec<DeliveryItem>,
    retries: Vec<RetryEntry>,
    locale: Locale,
}

impl DeliveryStatusEngine {
    pub fn new(items: Vec<DeliveryItem>) -> Self {
        Self {
            items,
            retries: Vec::new(),
            locale: Locale::English,
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

    fn build_screen(&self) -> ScreenModel {
        let recent: Vec<&DeliveryItem> = self
            .items
            .iter()
            .filter(|i| !matches!(i.status, Status::Failed | Status::Warning))
            .collect();
        let failed: Vec<&DeliveryItem> = self.items.iter().filter(|i| i.retryable).collect();

        let any_data = !self.items.is_empty() || !self.retries.is_empty();

        let mut components: Vec<Component> = Vec::new();

        if !any_data {
            components.push(Component::InfoPanel {
                id: "empty".into(),
                icon: Some("checkmark".into()),
                title: self.t("delivery_status.all_delivered_title"),
                items: vec![],
                a11y: None,
            });
        } else {
            // Section: Recent
            if !recent.is_empty() {
                components.push(section_header(
                    "section_recent",
                    &self.t("delivery_status.recent_section"),
                ));
                for item in &recent {
                    components.push(status_indicator_for(item, self.locale));
                }
            }
            // Section: Failed (with per-row retry actions)
            if !failed.is_empty() {
                if !components.is_empty() {
                    components.push(Component::Divider);
                }
                components.push(section_header(
                    "section_failed",
                    &get_string_with_args(
                        self.locale,
                        "delivery_status.failed_section",
                        &[("count", &failed.len().to_string())],
                    ),
                ));
                for item in &failed {
                    components.push(status_indicator_for(item, self.locale));
                }
            }
            // Section: Pending Retries
            if !self.retries.is_empty() {
                if !components.is_empty() {
                    components.push(Component::Divider);
                }
                components.push(section_header(
                    "section_pending",
                    &self.t("delivery_status.pending_retries_section"),
                ));
                for retry in &self.retries {
                    components.push(retry_indicator(retry, self.locale));
                }
            }
        }

        let actions = if self.items.iter().any(|item| item.retryable) {
            vec![ScreenAction {
                id: RETRY_ALL_ACTION_ID.into(),
                label: self.t("delivery_status.retry_failed_button"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }]
        } else {
            vec![]
        };

        ScreenModel {
            screen_id: "delivery_status".into(),
            title: self.t("delivery_status.title"),
            subtitle: None,
            components,
            contextual_actions: actions,
            progress: None,
            ..Default::default()
        }
    }
}

fn section_header(id: &str, label: &str) -> Component {
    Component::Text {
        a11y: None,
        id: id.into(),
        content: label.into(),
        style: TextStyle::Subtitle,
    }
}

fn status_indicator_for(item: &DeliveryItem, locale: Locale) -> Component {
    Component::StatusIndicator {
        id: item.message_id.clone(),
        icon: None,
        title: item.contact_name.clone(),
        detail: item.detail.clone(),
        status: item.status,
        status_label: get_string(locale, item.status.label_key()),
        a11y: None,
    }
}

fn retry_indicator(retry: &RetryEntry, locale: Locale) -> Component {
    let detail = if retry.max_exceeded {
        Some(get_string_with_args(
            locale,
            "delivery_status.max_attempts_exceeded",
            &[("max", &retry.max_attempts.to_string())],
        ))
    } else {
        Some(get_string_with_args(
            locale,
            "delivery_status.attempt_of",
            &[
                ("attempt", &retry.attempt.to_string()),
                ("max", &retry.max_attempts.to_string()),
            ],
        ))
    };
    let status = if retry.max_exceeded {
        Status::Failed
    } else {
        Status::Pending
    };
    Component::StatusIndicator {
        id: format!("pending:{}", retry.message_id),
        icon: None,
        title: retry.contact_name.clone(),
        detail,
        status,
        status_label: get_string(locale, status.label_key()),
        a11y: None,
    }
}

impl WorkflowEngine for DeliveryStatusEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemSelected {
                component_id: _,
                item_id,
            } => ActionResult::OpenContact {
                contact_id: item_id,
            },
            UserAction::ActionPressed { action_id } if action_id == RETRY_ALL_ACTION_ID => {
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
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: covers private build_screen helpers (section
// partitioning) without leaking them to pub API. Cross-crate integration
// in vauchi-core/tests/it/delivery_engine_tests.rs covers the public surface.
#[cfg(test)]
mod tests {
    use super::*;

    fn delivered(name: &str) -> DeliveryItem {
        DeliveryItem {
            message_id: format!("msg-{name}"),
            contact_id: format!("c-{name}"),
            contact_name: name.into(),
            status: Status::Success,
            detail: None,
            retryable: false,
        }
    }

    fn failed(name: &str) -> DeliveryItem {
        DeliveryItem {
            message_id: format!("msg-{name}"),
            contact_id: format!("c-{name}"),
            contact_name: name.into(),
            status: Status::Failed,
            detail: Some("network error".into()),
            retryable: true,
        }
    }

    fn retry(name: &str, attempt: u32, max: u32) -> RetryEntry {
        RetryEntry {
            message_id: format!("msg-{name}"),
            contact_id: format!("c-{name}"),
            contact_name: name.into(),
            attempt,
            max_attempts: max,
            max_exceeded: attempt >= max,
        }
    }

    // @internal
    #[test]
    fn empty_engine_emits_all_delivered_panel() {
        let engine = DeliveryStatusEngine::new(vec![]);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "delivery_status");
        assert_eq!(screen.title, "Delivery Status");
        assert_eq!(screen.components.len(), 1);
        assert!(matches!(
            &screen.components[0],
            Component::InfoPanel { title, .. } if title == "All Delivered"
        ));
        assert!(screen.contextual_actions.is_empty());
    }

    // @internal
    #[test]
    fn all_delivered_emits_recent_section_only() {
        let engine = DeliveryStatusEngine::new(vec![delivered("alice"), delivered("bob")]);
        let screen = engine.current_screen();
        // 1 header + 2 indicators = 3 components, no retry_all action
        assert_eq!(screen.components.len(), 3);
        assert!(matches!(
            &screen.components[0],
            Component::Text { content, .. } if content == "Recent"
        ));
        assert!(screen.contextual_actions.is_empty());
    }

    // @internal
    #[test]
    fn failed_records_emit_failed_section_and_retry_all_action() {
        let engine = DeliveryStatusEngine::new(vec![failed("alice"), failed("bob")]);
        let screen = engine.current_screen();
        // header + 2 indicators
        assert_eq!(screen.components.len(), 3);
        assert!(matches!(
            &screen.components[0],
            Component::Text { content, .. } if content == "Failed (2)"
        ));
        match &screen.components[1] {
            Component::StatusIndicator { id, status, .. } => {
                assert_eq!(id, "msg-alice");
                assert_eq!(*status, Status::Failed);
            }
            other => panic!("expected StatusIndicator, got {other:?}"),
        }
        // Footer: "Retry Failed" (single global action)
        assert_eq!(screen.contextual_actions.len(), 1);
        assert_eq!(screen.contextual_actions[0].id, RETRY_ALL_ACTION_ID);
    }

    // @internal
    #[test]
    fn pending_retries_emit_pending_section() {
        let engine = DeliveryStatusEngine::new(vec![]).with_retries(vec![retry("alice", 2, 5)]);
        let screen = engine.current_screen();
        assert_eq!(screen.components.len(), 2);
        assert!(matches!(
            &screen.components[0],
            Component::Text { content, .. } if content == "Pending Retries"
        ));
        match &screen.components[1] {
            Component::StatusIndicator { detail, status, .. } => {
                assert_eq!(detail.as_deref(), Some("Attempt 2 of 5"));
                assert_eq!(*status, Status::Pending);
            }
            other => panic!("expected StatusIndicator, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn max_exceeded_retry_marked_failed() {
        let engine = DeliveryStatusEngine::new(vec![]).with_retries(vec![retry("alice", 5, 5)]);
        let screen = engine.current_screen();
        match &screen.components[1] {
            Component::StatusIndicator { detail, status, .. } => {
                assert_eq!(detail.as_deref(), Some("Max attempts (5) exceeded"));
                assert_eq!(*status, Status::Failed);
            }
            other => panic!("expected StatusIndicator, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn mixed_state_emits_all_three_sections_with_dividers() {
        let engine = DeliveryStatusEngine::new(vec![delivered("alice"), failed("bob")])
            .with_retries(vec![retry("carol", 1, 3)]);
        let screen = engine.current_screen();
        let ids: Vec<String> = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::Text { content, .. } => Some(content.clone()),
                Component::Divider => Some("---".into()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                "Recent".to_string(),
                "---".into(),
                "Failed (1)".into(),
                "---".into(),
                "Pending Retries".into(),
            ]
        );
        assert_eq!(screen.contextual_actions.len(), 1);
    }

    // @internal
    #[test]
    fn retry_all_action_returns_retry_failed_deliveries() {
        let mut engine =
            DeliveryStatusEngine::new(vec![failed("alice"), delivered("bob"), failed("carol")]);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: RETRY_ALL_ACTION_ID.into(),
        });
        match result {
            ActionResult::RetryFailedDeliveries { message_ids } => {
                assert_eq!(
                    message_ids,
                    vec!["msg-alice".to_string(), "msg-carol".into()]
                );
            }
            other => panic!("expected RetryFailedDeliveries, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn retry_all_action_with_no_failed_returns_update_screen() {
        let mut engine = DeliveryStatusEngine::new(vec![delivered("alice")]);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: RETRY_ALL_ACTION_ID.into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn list_item_selected_routes_to_open_contact() {
        let mut engine = DeliveryStatusEngine::new(vec![delivered("alice")]);
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "section_recent".into(),
            item_id: "c-alice".into(),
        });
        match result {
            ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "c-alice"),
            other => panic!("expected OpenContact, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn adversarial_retry_ids_do_not_panic() {
        let mut engine = DeliveryStatusEngine::new(vec![]);
        for case in &[
            "",
            "retry:",
            "retry::::",
            "retry:🦀",
            "retry:'; DROP TABLE--",
        ] {
            let result = engine.handle_action(UserAction::ActionPressed {
                action_id: (*case).into(),
            });
            assert!(matches!(result, ActionResult::UpdateScreen(_)));
        }
    }
}

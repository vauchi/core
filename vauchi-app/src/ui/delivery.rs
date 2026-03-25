// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery status engine — shows delivery state per contact.

use crate::ui::*;

/// A single delivery item with status and retry info.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeliveryItem {
    pub contact_id: String,
    pub contact_name: String,
    pub status: Status,
    pub detail: Option<String>,
    pub retryable: bool,
}

/// Engine that displays delivery status for a set of contacts.
#[derive(Clone, Debug)]
pub struct DeliveryStatusEngine {
    items: Vec<DeliveryItem>,
}

impl DeliveryStatusEngine {
    pub fn new(items: Vec<DeliveryItem>) -> Self {
        Self { items }
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.items.is_empty() {
            vec![Component::InfoPanel {
                id: "empty".into(),
                icon: Some("checkmark".into()),
                title: "All Delivered".into(),
                items: vec![],
                accessible_label: None,
                accessible_hint: None,
            }]
        } else {
            self.items
                .iter()
                .map(|item| Component::StatusIndicator {
                    id: item.contact_id.clone(),
                    icon: None,
                    title: item.contact_name.clone(),
                    detail: item.detail.clone(),
                    status: item.status.clone(),
                    accessible_label: None,
                    accessible_hint: None,
                })
                .collect()
        };

        let actions = if self.items.iter().any(|item| item.retryable) {
            vec![ScreenAction {
                id: "retry_all".into(),
                label: "Retry Failed".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }]
        } else {
            vec![]
        };

        ScreenModel {
            screen_id: "delivery_status".into(),
            title: "Delivery Status".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
        }
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
            UserAction::ActionPressed { action_id } if action_id == "retry_all" => {
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

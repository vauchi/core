// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Help & FAQ engine — single screen listing help items grouped by category.

use crate::ui::*;

/// A single help/FAQ item.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HelpItem {
    pub id: String,
    pub question: String,
    /// Inline answer text (preferred for TUI / offline use).
    pub answer: Option<String>,
    /// External URL (used by mobile/desktop if no inline answer, or as "Learn more" link).
    pub answer_url: Option<String>,
    pub category: String,
}

/// Help & FAQ engine — displays help items grouped by category.
#[derive(Clone, Debug)]
pub struct HelpEngine {
    items: Vec<HelpItem>,
    search_query: String,
}

impl HelpEngine {
    pub fn new(items: Vec<HelpItem>) -> Self {
        Self {
            items,
            search_query: String::new(),
        }
    }

    /// Returns items filtered by the current search query.
    fn filtered_items(&self) -> Vec<&HelpItem> {
        if self.search_query.is_empty() {
            return self.items.iter().collect();
        }
        let query = self.search_query.to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                item.question.to_lowercase().contains(&query)
                    || item
                        .answer
                        .as_deref()
                        .is_some_and(|a| a.to_lowercase().contains(&query))
            })
            .collect()
    }

    /// Returns the unique categories from filtered items in first-appearance order.
    fn categories(&self, filtered: &[&HelpItem]) -> Vec<String> {
        let mut seen = Vec::new();
        for item in filtered {
            if !seen.contains(&item.category) {
                seen.push(item.category.clone());
            }
        }
        seen
    }
}

impl WorkflowEngine for HelpEngine {
    fn current_screen(&self) -> ScreenModel {
        let filtered = self.filtered_items();

        let mut components: Vec<Component> = vec![Component::TextInput {
            id: "help_search".into(),
            label: "Search".into(),
            value: self.search_query.clone(),
            placeholder: Some("Search help topics…".into()),
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: None,
            info_key: None,
        }];

        for category in self.categories(&filtered) {
            let items = filtered
                .iter()
                .filter(|item| item.category == category)
                .map(|item| ActionListItem {
                    id: item.id.clone(),
                    label: item.question.clone(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                })
                .collect();

            components.push(Component::ActionList {
                id: category,
                items,
            });
        }

        ScreenModel {
            screen_id: "help".into(),
            title: "Help & FAQ".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "help_search" => {
                self.search_query = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { item_id, .. } => {
                if let Some(item) = self.items.iter().find(|i| i.id == item_id) {
                    // Prefer inline answer (works in TUI and offline)
                    if let Some(ref answer) = item.answer {
                        return ActionResult::ShowInfoOverlay {
                            title: item.question.clone(),
                            body: answer.clone(),
                        };
                    }
                    // Fall back to URL for items without inline text
                    if let Some(ref url) = item.answer_url {
                        return ActionResult::OpenUrl { url: url.clone() };
                    }
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

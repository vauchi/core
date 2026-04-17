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
}

impl HelpEngine {
    pub fn new(items: Vec<HelpItem>) -> Self {
        Self { items }
    }

    /// Returns the unique categories in the order they first appear.
    fn categories(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for item in &self.items {
            if !seen.contains(&item.category) {
                seen.push(item.category.clone());
            }
        }
        seen
    }
}

impl WorkflowEngine for HelpEngine {
    fn current_screen(&self) -> ScreenModel {
        let components: Vec<Component> = self
            .categories()
            .into_iter()
            .map(|category| {
                let items = self
                    .items
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

                Component::ActionList {
                    id: category,
                    items,
                }
            })
            .collect();

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
            UserAction::ListItemSelected { item_id, .. } => {
                if let Some(item) = self.items.iter().find(|i| i.id == item_id) {
                    // Prefer inline answer (works in TUI and offline)
                    if let Some(ref answer) = item.answer {
                        return ActionResult::ShowAlert {
                            title: item.question.clone(),
                            message: answer.clone(),
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

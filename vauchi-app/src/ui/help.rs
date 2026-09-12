// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Help & FAQ engine — single screen: a search field over the help
//! items, grouped into labelled sections (the design canvas's FAQ
//! HIGHLIGHTS, the remaining questions, and MORE).

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// Section ids the catalog files items under. A section whose id is
/// none of these renders its id verbatim as the header, so ad-hoc
/// catalogs (tests, content updates) still group.
pub const SECTION_HIGHLIGHTS: &str = "highlights";
pub const SECTION_FAQ: &str = "faq";
pub const SECTION_MORE: &str = "more";

/// A single help/FAQ item.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HelpItem {
    pub id: String,
    pub question: String,
    /// One-line description under the question (a MORE row's "what
    /// happens when you tap"). Serde-defaulted so older catalogs decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Inline answer text (preferred for TUI / offline use).
    pub answer: Option<String>,
    /// External URL (used by mobile/desktop if no inline answer, or as "Learn more" link).
    pub answer_url: Option<String>,
    /// Section id; see [`SECTION_HIGHLIGHTS`] and friends.
    pub category: String,
}

/// Help & FAQ engine — displays help items grouped by section.
#[derive(Clone, Debug)]
pub struct HelpEngine {
    items: Vec<HelpItem>,
    search_query: String,
    locale: Locale,
}

impl HelpEngine {
    pub fn new(items: Vec<HelpItem>) -> Self {
        Self {
            items,
            search_query: String::new(),
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
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

    /// Returns the unique sections from filtered items in first-appearance order.
    fn sections(&self, filtered: &[&HelpItem]) -> Vec<String> {
        let mut seen = Vec::new();
        for item in filtered {
            if !seen.contains(&item.category) {
                seen.push(item.category.clone());
            }
        }
        seen
    }

    fn section_label(&self, section: &str) -> String {
        match section {
            SECTION_HIGHLIGHTS => get_string(self.locale, "help.faq_highlights"),
            SECTION_FAQ => get_string(self.locale, "help.more_questions"),
            SECTION_MORE => get_string(self.locale, "help.more"),
            other => other.to_string(),
        }
    }

    fn section(&self, id: &str, filtered: &[&HelpItem]) -> Section {
        let items = filtered
            .iter()
            .filter(|item| item.category == id)
            .map(|item| ActionListItem {
                id: item.id.clone(),
                label: item.question.clone(),
                icon: None,
                detail: item.subtitle.clone(),
                a11y: None,
                info_key: None,
            })
            .collect();
        Section {
            id: id.into(),
            label: self.section_label(id),
            items,
        }
    }
}

impl WorkflowEngine for HelpEngine {
    fn current_screen(&self) -> ScreenModel {
        let filtered = self.filtered_items();

        let mut components: Vec<Component> = vec![Component::TextInput {
            id: "help_search".into(),
            label: get_string(self.locale, "help.search_button"),
            value: self.search_query.clone(),
            placeholder: Some(get_string(self.locale, "help.search_placeholder")),
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: None,
            info_key: None,
        }];

        let sections: Vec<Section> = self
            .sections(&filtered)
            .iter()
            .map(|id| self.section(id, &filtered))
            .collect();
        if !sections.is_empty() {
            components.push(Component::SectionedActionList {
                id: "help".into(),
                sections,
            });
        }

        ScreenModel {
            screen_id: "help".into(),
            title: get_string(self.locale, "help.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
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

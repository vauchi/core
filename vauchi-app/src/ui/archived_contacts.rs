// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Archived contacts engine — view and unarchive previously archived contacts.

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// Engine that displays archived contacts and allows unarchiving them.
#[derive(Clone, Debug)]
pub struct ArchivedContactsEngine {
    /// (contact_id, display_name) pairs for archived contacts.
    contacts: Vec<(String, String)>,
    locale: Locale,
}

impl ArchivedContactsEngine {
    pub fn new(contacts: Vec<(String, String)>) -> Self {
        Self {
            contacts,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.contacts.is_empty() {
            vec![Component::Text {
                id: "no_archived".into(),
                content: self.t("archived_contacts.empty"),
                style: TextStyle::Body,
            }]
        } else {
            vec![Component::ActionList {
                id: "archived_contacts".into(),
                items: self
                    .contacts
                    .iter()
                    .map(|(id, name)| ActionListItem {
                        id: format!("unarchive_{id}"),
                        label: name.clone(),
                        icon: None,
                        detail: Some(self.t("archived_contacts.tap_to_unarchive")),
                        a11y: None,
                        info_key: None,
                    })
                    .collect(),
            }]
        };

        ScreenModel {
            screen_id: "archived_contacts".into(),
            title: self.t("archived_contacts.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for ArchivedContactsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id.starts_with("unarchive_") => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

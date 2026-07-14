// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fallback contact-detail workflow for an unknown contact id.

use super::*;

/// Fallback engine for when a contact is not found.
#[derive(Clone, Debug)]
pub struct ContactNotFoundEngine {
    contact_id: String,
    locale: Locale,
}

impl ContactNotFoundEngine {
    pub fn new(contact_id: String) -> Self {
        Self {
            contact_id,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-12).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }
}

impl WorkflowEngine for ContactNotFoundEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_not_found".into(),
            title: self.t("contact_detail.not_found_title"),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "not_found".into(),
                icon: None,
                title: self.t("contact_detail.not_found_status"),
                items: vec![InfoItem {
                    icon: None,
                    title: self.t("status.error"),
                    detail: get_string_with_args(
                        self.locale,
                        "contact_detail.not_found_detail",
                        &[("id", &self.contact_id)],
                    ),
                }],
                a11y: Some(A11y {
                    label: Some(self.t("contact_detail.not_found_status")),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![ScreenAction {
                id: "back".into(),
                label: self.t("action.back"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.back"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

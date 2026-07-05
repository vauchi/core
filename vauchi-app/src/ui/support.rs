// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Support engine — links to support and sponsor the project.

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// Engine that displays support and sponsorship information.
#[derive(Clone, Debug)]
pub struct SupportEngine {
    locale: Locale,
}

impl Default for SupportEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SupportEngine {
    pub fn new() -> Self {
        Self {
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
        ScreenModel {
            screen_id: "support".into(),
            title: self.t("support.title"),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "support_info".into(),
                icon: Some("heart".into()),
                title: self.t("support.info_title"),
                items: vec![
                    InfoItem {
                        icon: Some("github".into()),
                        title: self.t("support.github_sponsors_label"),
                        detail: "https://github.com/sponsors/vauchi".into(),
                    },
                    InfoItem {
                        icon: Some("liberapay".into()),
                        title: self.t("support.liberapay_label"),
                        detail: "https://liberapay.com/vauchi".into(),
                    },
                ],
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "open_github_sponsors".into(),
                    label: self.t("support.github_sponsors_label"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "open_liberapay".into(),
                    label: self.t("support.liberapay_label"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for SupportEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "open_github_sponsors" => ActionResult::OpenUrl {
                    url: "https://github.com/sponsors/vauchi".into(),
                },
                "open_liberapay" => ActionResult::OpenUrl {
                    url: "https://liberapay.com/vauchi".into(),
                },
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

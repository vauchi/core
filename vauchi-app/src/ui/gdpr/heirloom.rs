// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The paper heirloom's plaintext-warning screen (private#363).

use super::GdprEngine;
use crate::ui::*;

impl GdprEngine {
    /// The export leaves the encryption envelope, so nothing is written
    /// until the person confirms the warning (private#363).
    pub(super) fn build_confirm_heirloom(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "confirm_heirloom".into(),
            title: self.t("privacy.heirloom.title"),
            subtitle: Some(self.t("privacy.heirloom.subtitle")),
            components: vec![Component::InfoPanel {
                id: "heirloom_warning".into(),
                icon: Some("warning".into()),
                title: self.t("privacy.heirloom.warning_title"),
                items: vec![InfoItem {
                    icon: Some("warning".into()),
                    title: self.t("privacy.heirloom.warning_title"),
                    detail: self.t("privacy.heirloom.warning_detail"),
                }],
                a11y: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "confirm_heirloom".into(),
                    label: self.t("privacy.heirloom.confirm"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("privacy.heirloom.confirm"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

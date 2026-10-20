// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The PIN step in front of every data export (#712): the export waits
//! here until the AppEngine confirms the session PIN, and one confirmation
//! covers one export.

use super::*;
use crate::ui::device_link_pin::{PinStepCopy, confirm_pin_screen_for};

pub(crate) const SET_APP_PASSWORD_ACTION_ID: &str = "set_app_password";

impl GdprEngine {
    pub(super) fn begin_export(&mut self, kind: &'static str) -> ActionResult {
        self.last_action = None;
        self.pending_export = Some(kind);
        self.entered_pin.clear();
        self.pin_error = None;
        self.step = if self.app_password_set {
            GdprStep::ConfirmExportPin
        } else {
            GdprStep::ExportNeedsAppPassword
        };
        ActionResult::NavigateTo(self.build_screen())
    }

    pub(super) fn abandon_export(&mut self) {
        self.pending_export = None;
        self.entered_pin.clear();
        self.pin_error = None;
    }

    pub(super) fn apply_export_update(&mut self, update: GdprExportUpdate) -> bool {
        if !matches!(self.step, GdprStep::ConfirmExportPin) {
            return false;
        }
        match update {
            GdprExportUpdate::Verified => {
                self.last_action = self.pending_export.take().map(str::to_owned);
                self.entered_pin.clear();
                self.pin_error = None;
                self.step = GdprStep::Overview;
            }
            GdprExportUpdate::PinRejected => {
                self.entered_pin.clear();
                self.pin_error = Some(self.t("lock_screen.wrong_password"));
            }
        }
        true
    }

    pub(super) fn build_confirm_export_pin(&self) -> ScreenModel {
        confirm_pin_screen_for(
            self.locale,
            PinStepCopy {
                screen_id: "export_confirm_pin",
                title_key: "privacy.export_data",
                hint_key: "privacy.export_confirm_pin_hint",
            },
            self.entered_pin.as_str(),
            self.pin_error.as_deref(),
        )
    }

    pub(super) fn build_export_needs_app_password(&self) -> ScreenModel {
        let body = self.t("privacy.export_needs_password_body");
        ScreenModel {
            screen_id: "export_needs_app_password".into(),
            title: self.t("privacy.export_needs_password_title"),
            components: vec![Component::Text {
                id: "export_needs_app_password_body".into(),
                content: body.clone(),
                style: TextStyle::Body,
                a11y: Some(A11y::labeled(body)),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: SET_APP_PASSWORD_ACTION_ID.into(),
                    label: self.t("privacy.set_app_password"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            ..Default::default()
        }
    }
}

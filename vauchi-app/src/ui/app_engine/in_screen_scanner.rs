// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A scan with no shell to run it. `Command::QrRequestScan` reaches the
//! hardware camera only once a hardware shell is attached (ADR-077; Linux
//! today). Without one it reached display shells that have no handler for
//! it, the Android and iOS apps, and every scan button there did nothing
//! (vauchi/private#1365, #1382). Core then shows the exchange's scanner in
//! the screen that asked, and a scanned value takes the path a hardware
//! scan takes (owner, 2026-11-24: Core picks by capability).

use vauchi_core::{Command, Event};

use super::AppEngine;
use crate::ui::{
    A11y, ActionResult, ActionStyle, Component, QrMode, ScreenAction, ScreenModel, UserAction,
    WorkflowEngine,
};

const SCANNER_ID: &str = "in_screen_scanner";
const LEAVE_SCANNER_ID: &str = "leave_scanner";

impl AppEngine {
    /// After an engine's answer: a scan request no shell can run opens the
    /// scanner on the screen that asked, and a screen change closes it.
    pub(super) fn scan_in_screen_without_a_hardware_shell(
        &mut self,
        result: ActionResult,
    ) -> ActionResult {
        let shown = self.engine.current_screen().screen_id;
        if self
            .in_screen_scanner
            .as_ref()
            .is_some_and(|id| *id != shown)
        {
            self.in_screen_scanner = None;
        }
        if self.hardware_route.report().is_some() {
            return result;
        }
        let ActionResult::Commands { commands } = result else {
            return result;
        };
        if !commands.contains(&Command::QrRequestScan) {
            return ActionResult::Commands { commands };
        }
        self.in_screen_scanner = Some(shown);
        let commands: Vec<_> = commands
            .into_iter()
            .filter(|command| *command != Command::QrRequestScan)
            .collect();
        if commands.is_empty() {
            ActionResult::UpdateScreen(self.current_screen())
        } else {
            ActionResult::Commands { commands }
        }
    }

    /// The scanner's value is a scan, and Leave closes it. Both reach Core
    /// before the screen's engine, which knows nothing of the scanner. Like
    /// a hardware scan, one tap takes one code: a value Core acts on closes
    /// the scanner, and one it ignores leaves it looking.
    pub(super) fn intercept_in_screen_scanner(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        self.in_screen_scanner.as_ref()?;
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == SCANNER_ID => {
                let scanned = Event::QrScanned {
                    data: value.trim().to_string(),
                };
                match self.handle_hardware_event(scanned) {
                    Some(result) => {
                        self.in_screen_scanner = None;
                        Some(result)
                    }
                    None => Some(ActionResult::UpdateScreen(self.current_screen())),
                }
            }
            UserAction::ActionPressed { action_id } if action_id == LEAVE_SCANNER_ID => {
                self.in_screen_scanner = None;
                Some(ActionResult::UpdateScreen(self.current_screen()))
            }
            _ => None,
        }
    }

    pub(super) fn apply_in_screen_scanner(&self, mut screen: ScreenModel) -> ScreenModel {
        if self.in_screen_scanner.as_deref() != Some(screen.screen_id.as_str()) {
            return screen;
        }
        let label = self.t("exchange.ble.glance_scan");
        screen.components.insert(
            0,
            Component::QrCode {
                id: SCANNER_ID.into(),
                data: String::new(),
                frames: Vec::new(),
                mode: QrMode::Scan,
                label: Some(label.clone()),
                scan_quality: None,
                placement: None,
                error_correction: None,
                size: None,
                a11y: Some(A11y::labeled(label)),
            },
        );
        let leave = self.t("action.cancel");
        screen.contextual_actions.push(ScreenAction {
            id: LEAVE_SCANNER_ID.into(),
            label: leave.clone(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(leave)),
        });
        screen
    }
}

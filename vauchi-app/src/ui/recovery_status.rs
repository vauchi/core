// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social recovery engine — shows quorum status and trusted contacts.

use crate::ui::*;

/// Engine that displays social recovery status.
#[derive(Clone, Debug)]
pub struct RecoveryEngine {
    trusted_contacts: Vec<ContactItem>,
    quorum_threshold: usize,
}

impl RecoveryEngine {
    pub fn new(trusted_contacts: Vec<ContactItem>, quorum_threshold: usize) -> Self {
        Self {
            trusted_contacts,
            quorum_threshold,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let current = self.trusted_contacts.len();
        let quorum_met = current >= self.quorum_threshold;

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: "Social Recovery".into(),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "quorum_info".into(),
                    icon: Some("recovery".into()),
                    title: "Quorum Status".into(),
                    items: vec![
                        InfoItem {
                            icon: None,
                            title: "Trusted Contacts".into(),
                            detail: format!("{current} of {}", self.quorum_threshold),
                        },
                        InfoItem {
                            icon: None,
                            title: "Quorum Met".into(),
                            detail: if quorum_met { "Yes" } else { "No" }.into(),
                        },
                    ],
                    a11y: None,
                },
                Component::ContactList {
                    id: "trusted_contacts".into(),
                    contacts: self.trusted_contacts.clone(),
                    searchable: false,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "claim".into(),
                    label: "Start Recovery".into(),
                    style: ActionStyle::Primary,
                    enabled: quorum_met,
                },
                ScreenAction {
                    id: "status".into(),
                    label: "Check Status".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for RecoveryEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "claim" => ActionResult::ShowAlert {
                    title: "Coming Soon".into(),
                    message: "Social recovery will be available in a future update.".into(),
                },
                "status" => ActionResult::ShowAlert {
                    title: "Recovery Status".into(),
                    message: "No active recovery claims.".into(),
                },
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A restore imports the whole backup, about 90 s for 10k contacts on a
//! Pixel 3a. Run inside the call that handled the submit, it left the
//! password page up, unchanged and tappable, until it was done
//! (vauchi/private#273). The submit now only records the restore and shows
//! that it is running; the next wakeup, asked for at once, runs it.

use zeroize::Zeroizing;

use super::AppEngine;
use crate::ui::{A11y, ActionResult, Component, ScreenModel, Status, WorkflowEngine};

pub(super) struct PendingRestore {
    bytes: Vec<u8>,
    password: Zeroizing<String>,
}

impl AppEngine {
    pub(super) fn defer_backup_restore(
        &mut self,
        bytes: Vec<u8>,
        password: String,
    ) -> ActionResult {
        self.pending_restore = Some(PendingRestore {
            bytes,
            password: Zeroizing::new(password),
        });
        self.pending_commands.push_back(self.compute_next_wakeup());
        ActionResult::UpdateScreen(self.current_screen())
    }

    pub(super) fn restore_pending(&self) -> bool {
        self.pending_restore.is_some()
    }

    /// Runs the recorded restore and queues what it ends in: the restored
    /// app is presented by the caller's re-present, an alert rides the
    /// pending commands.
    pub(super) fn run_pending_restore(&mut self) {
        let Some(PendingRestore { bytes, password }) = self.pending_restore.take() else {
            return;
        };
        let result = self.execute_backup_restore(bytes, &password);
        let mut commands = Vec::new();
        if let Err(variant) = super::result_commands::append_result_commands(result, &mut commands)
        {
            tracing::warn!(variant, "restore result has no command");
        }
        self.pending_commands.extend(commands);
    }

    pub(super) fn apply_restore_in_progress(&self, screen: ScreenModel) -> ScreenModel {
        if !self.restore_pending() {
            return screen;
        }
        let title = self.t("backup.wizard.restoring_title");
        let detail = self.t("backup.wizard.restoring_detail");
        ScreenModel {
            screen_id: "restore_in_progress".into(),
            title: title.clone(),
            components: vec![Component::StatusIndicator {
                id: "restore_status".into(),
                icon: None,
                title: title.clone(),
                detail: Some(detail.clone()),
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(title),
                    hint: Some(detail),
                    role: None,
                }),
            }],
            contextual_actions: vec![],
            ..Default::default()
        }
    }
}

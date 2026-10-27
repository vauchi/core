// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{AppEngine, AppScreen};
use vauchi_core::{Command, Event, FilePickOutcome, FilePickPurpose, MAX_FILE_PICK_BYTES};

pub(super) struct PendingFilePick {
    request_id: String,
    screen: AppScreen,
    contact_import: bool,
}

impl AppEngine {
    pub(super) fn cancel_file_pick(&mut self) {
        self.pending_contact_import = false;
        if let Some(pending) = self.pending_file_pick.take() {
            self.pending_commands.push_back(Command::CancelFilePick {
                request_id: pending.request_id,
            });
        }
    }

    pub(super) fn invalidate_file_pick_for_navigation(&mut self, screen: &AppScreen) {
        if self.pending_file_pick.as_ref().is_some_and(|pending| {
            matches!(screen, AppScreen::Lock)
                || (!pending.contact_import && pending.screen != *screen)
        }) {
            self.cancel_file_pick();
        }
    }

    pub(super) fn correlate_file_commands(&mut self, commands: Vec<Command>) -> Vec<Command> {
        if !self.correlated_file_picker {
            return commands;
        }
        let mut output = Vec::with_capacity(commands.len());
        for command in commands {
            let Command::FilePickFromUser {
                accepted_mime_types,
                accepted_extensions,
                max_bytes,
                purpose,
            } = command
            else {
                output.push(command);
                continue;
            };
            if let Some(previous) = self.pending_file_pick.take() {
                output.push(Command::CancelFilePick {
                    request_id: previous.request_id,
                });
            }
            // Independent ids prevent a delayed callback surviving an engine restart (#764).
            let request_id = hex::encode(vauchi_core::crypto::random_bytes::<16>());
            let title = match purpose {
                FilePickPurpose::ImportContacts => self.t("menu.import_contacts"),
                FilePickPurpose::ImportBackup => self.t("backup.import_title"),
                FilePickPurpose::Other { label_key } => self.t(&label_key),
                _ => self.t("menu.file"),
            };
            self.pending_file_pick = Some(PendingFilePick {
                request_id: request_id.clone(),
                screen: self.screen.clone(),
                contact_import: self.pending_contact_import,
            });
            output.push(Command::PickFile {
                request_id,
                title,
                accepted_mime_types,
                accepted_extensions,
                max_bytes: max_bytes
                    .unwrap_or(MAX_FILE_PICK_BYTES as u64)
                    .min(MAX_FILE_PICK_BYTES as u64),
            });
        }
        output
    }

    pub(super) fn correlate_file_event(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::FilePickerCapabilitiesChanged { correlated_results } => {
                if self.correlated_file_picker != correlated_results {
                    self.cancel_file_pick();
                    self.correlated_file_picker = correlated_results;
                }
                None
            }
            Event::FilePickCompleted {
                request_id,
                outcome,
            } => {
                let pending = self.pending_file_pick.as_ref()?;
                if pending.request_id != request_id {
                    return None;
                }
                if self.session_locked || (!pending.contact_import && pending.screen != self.screen)
                {
                    self.cancel_file_pick();
                    return None;
                }
                self.pending_file_pick = None;
                match outcome {
                    FilePickOutcome::Selected { bytes, filename } => {
                        if bytes.len() > MAX_FILE_PICK_BYTES {
                            self.pending_contact_import = false;
                            return Some(Event::HardwareError {
                                transport: "file_picker".into(),
                                error: String::new(),
                            });
                        }
                        Some(Event::FilePickedFromUser { bytes, filename })
                    }
                    FilePickOutcome::Cancelled => Some(Event::FilePickCancelledByUser),
                    FilePickOutcome::Failed { .. } => Some(Event::HardwareError {
                        transport: "file_picker".into(),
                        error: String::new(),
                    }),
                }
            }
            Event::FilePickedFromUser { .. } | Event::FilePickCancelledByUser
                if self.correlated_file_picker =>
            {
                None
            }
            Event::HardwareError { ref transport, .. }
                if self.correlated_file_picker && transport == "file_picker" =>
            {
                None
            }
            other => Some(other),
        }
    }
}

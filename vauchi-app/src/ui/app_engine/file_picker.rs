// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{AppEngine, AppScreen};
use vauchi_core::{Command, Event, FilePickOutcome, FilePickPurpose, MAX_FILE_PICK_BYTES};

pub(super) struct PendingFilePick {
    request_id: String,
    screen: AppScreen,
    contact_import: bool,
    answer: PickAnswer,
}

/// The event a completed pick is reported as to the screen that asked.
#[derive(Clone, Copy, PartialEq)]
enum PickAnswer {
    File,
    Image,
}

const IMAGE_MIME_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/bmp"];
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

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
        // A display that answers picks shows its own picker for the image
        // file too; the library request beside it would open a second one.
        let asks_for_an_image_file = commands.contains(&Command::ImagePickFromFile);
        let mut output = Vec::with_capacity(commands.len());
        for command in commands {
            match command {
                Command::FilePickFromUser {
                    accepted_mime_types,
                    accepted_extensions,
                    max_bytes,
                    purpose,
                } => {
                    let title = match purpose {
                        FilePickPurpose::ImportContacts => self.t("menu.import_contacts"),
                        FilePickPurpose::ImportBackup => self.t("backup.import_title"),
                        FilePickPurpose::Other { label_key } => self.t(&label_key),
                    };
                    self.ask_pick(
                        &mut output,
                        PickAnswer::File,
                        title,
                        accepted_mime_types,
                        accepted_extensions,
                        max_bytes,
                    );
                }
                Command::ImagePickFromFile => {
                    let title = self.t("avatar.choose_title");
                    self.ask_pick(
                        &mut output,
                        PickAnswer::Image,
                        title,
                        IMAGE_MIME_TYPES.map(String::from).to_vec(),
                        IMAGE_EXTENSIONS.map(String::from).to_vec(),
                        None,
                    );
                }
                Command::ImagePickFromLibrary if asks_for_an_image_file => {}
                other => output.push(other),
            }
        }
        output
    }

    fn ask_pick(
        &mut self,
        output: &mut Vec<Command>,
        answer: PickAnswer,
        title: String,
        accepted_mime_types: Vec<String>,
        accepted_extensions: Vec<String>,
        max_bytes: Option<u64>,
    ) {
        if let Some(previous) = self.pending_file_pick.take() {
            output.push(Command::CancelFilePick {
                request_id: previous.request_id,
            });
        }
        // Independent ids prevent a delayed callback surviving an engine restart (#764).
        let request_id = hex::encode(vauchi_core::crypto::random_bytes::<16>());
        self.pending_file_pick = Some(PendingFilePick {
            request_id: request_id.clone(),
            screen: self.screen.clone(),
            contact_import: answer == PickAnswer::File && self.pending_contact_import,
            answer,
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
                let answer = pending.answer;
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
                        Some(match answer {
                            PickAnswer::File => Event::FilePickedFromUser { bytes, filename },
                            PickAnswer::Image => Event::ImageReceived { data: bytes },
                        })
                    }
                    FilePickOutcome::Cancelled => Some(match answer {
                        PickAnswer::File => Event::FilePickCancelledByUser,
                        PickAnswer::Image => Event::ImagePickCancelled,
                    }),
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

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Edit Contact — one form for what is yours about a contact: the name you
//! see them by (a private nickname) and your personal note (#451). Their
//! entries are theirs and arrive with their card updates, so the retired
//! visibility and preview steps edited nothing that was ever saved.
//! No Storage or Vauchi dependency; the caller persists the changed values
//! from [`EngineOutput::ContactEdit`] when [`ActionResult::Complete`] is
//! returned.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

const NAME_ID: &str = "display_name";
const NOTE_ID: &str = "personal_note";
const NAME_ACTIONS_ID: &str = "name_actions";
const USE_CARD_NAME_ID: &str = "use_card_name";
const FORM_ACTIONS_ID: &str = "contact_edit_actions";
const SAVE_ID: &str = "save";
const DISCARD_CONFIRM_ID: &str = "discard_changes";
const KEEP_EDITING_ID: &str = "keep_editing";
/// `Vauchi::set_contact_nickname` rejects longer names.
const NAME_MAX_LENGTH: usize = 100;

/// What the form starts from.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EditableContact {
    /// The name on the contact's own card.
    pub card_name: String,
    /// The name you see them by: your nickname, or their card name.
    pub display_name: String,
    pub personal_note: String,
}

/// Engine for the single Edit Contact form.
#[derive(Clone, Debug)]
pub struct ContactEditEngine {
    original: EditableContact,
    name: String,
    note: String,
    name_error: Option<String>,
    confirming_discard: bool,
    leaving: Option<Leave>,
    locale: Locale,
}

/// How the form was left. Completion leaves through `navigate_back`, which
/// asks [`WorkflowEngine::navigate_back_within`] first, so the engine must
/// not ask about unsaved changes once the owner has chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leave {
    Save,
    Discard,
}

impl ContactEditEngine {
    pub fn new(contact: EditableContact) -> Self {
        Self {
            name: contact.display_name.clone(),
            note: contact.personal_note.clone(),
            original: contact,
            name_error: None,
            confirming_discard: false,
            leaving: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-13).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn name_changed(&self) -> bool {
        self.name.trim() != self.original.display_name
    }

    fn note_changed(&self) -> bool {
        self.note != self.original.personal_note
    }

    fn has_changes(&self) -> bool {
        self.name_changed() || self.note_changed()
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = vec![
            Component::TextInput {
                id: NAME_ID.into(),
                label: self.t("contact_edit.name_label"),
                value: self.name.clone(),
                placeholder: Some(self.t("contact_edit.enter_name_placeholder")),
                max_length: Some(NAME_MAX_LENGTH),
                validation_error: self.name_error.clone(),
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("contact_edit.name_label")),
                    hint: Some(self.t("contact_edit.name_hint")),
                    role: None,
                }),
                info_key: None,
            },
            Component::Text {
                id: "name_hint".into(),
                content: self.t("contact_edit.name_hint"),
                style: TextStyle::Caption,
                a11y: None,
            },
        ];

        if self.name.trim() != self.original.card_name {
            components.push(Component::ButtonList {
                id: NAME_ACTIONS_ID.into(),
                items: vec![ActionListItem {
                    id: USE_CARD_NAME_ID.into(),
                    label: get_string_with_args(
                        self.locale,
                        "contact_edit.use_card_name_button",
                        &[("name", &self.original.card_name)],
                    ),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                }],
            });
        }

        components.push(Component::TextInput {
            id: NOTE_ID.into(),
            label: self.t("contact_edit.note_label"),
            value: self.note.clone(),
            placeholder: Some(self.t("contact_edit.note_placeholder")),
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some(self.t("contact_edit.note_label")),
                hint: Some(self.t("contact_edit.note_placeholder")),
                role: None,
            }),
            info_key: None,
        });

        if self.confirming_discard {
            components.push(Component::InlineConfirm {
                id: DISCARD_CONFIRM_ID.into(),
                warning: self.t("contact_edit.discard_warning"),
                confirm_text: self.t("contact_edit.discard_button"),
                cancel_text: self.t("contact_edit.keep_editing_button"),
                confirm_action_id: DISCARD_CONFIRM_ID.into(),
                cancel_action_id: KEEP_EDITING_ID.into(),
                destructive: true,
                a11y: None,
            });
        }

        components.push(Component::ButtonList {
            id: FORM_ACTIONS_ID.into(),
            items: vec![ActionListItem {
                id: SAVE_ID.into(),
                label: self.t("action.save"),
                icon: None,
                detail: None,
                a11y: None,
                info_key: None,
            }],
        });

        ScreenModel {
            screen_id: "contact_edit".into(),
            title: self.t("contact_edit.edit_contact_title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn save(&mut self) -> ActionResult {
        if self.name.trim().is_empty() {
            let message = self.t("contact_edit.name_required_error");
            self.name_error = Some(message.clone());
            return ActionResult::ValidationError {
                component_id: NAME_ID.into(),
                message,
            };
        }
        self.leaving = Some(Leave::Save);
        ActionResult::Complete
    }

    fn asks_before_leaving(&self) -> bool {
        self.leaving.is_none() && self.has_changes() && !self.confirming_discard
    }
}

impl WorkflowEngine for ContactEditEngine {
    /// Only the values that changed, so Discard and an unchanged Save
    /// write nothing.
    fn engine_output(&self) -> Option<EngineOutput> {
        if self.leaving != Some(Leave::Save) {
            return Some(EngineOutput::ContactEdit {
                display_name: None,
                personal_note: None,
            });
        }
        Some(EngineOutput::ContactEdit {
            display_name: self.name_changed().then(|| self.name.trim().to_string()),
            personal_note: self.note_changed().then(|| self.note.clone()),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn can_navigate_back_within(&self) -> bool {
        self.asks_before_leaving()
    }

    /// The first Back with unsaved changes stays and asks; a second Back
    /// while asking leaves, as Discard would.
    fn navigate_back_within(&mut self) -> bool {
        if self.asks_before_leaving() {
            self.confirming_discard = true;
            return true;
        }
        self.leaving.get_or_insert(Leave::Discard);
        false
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == NAME_ID => {
                self.name = value;
                self.name_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == NOTE_ID => {
                self.note = value;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == NAME_ACTIONS_ID && item_id == USE_CARD_NAME_ID => {
                self.name = self.original.card_name.clone();
                self.name_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == FORM_ACTIONS_ID && item_id == SAVE_ID => self.save(),
            UserAction::ActionPressed { action_id } if action_id == DISCARD_CONFIRM_ID => {
                self.confirming_discard = false;
                self.leaving = Some(Leave::Discard);
                ActionResult::Complete
            }
            UserAction::ActionPressed { action_id } if action_id == KEEP_EDITING_ID => {
                self.confirming_discard = false;
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

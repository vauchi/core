// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoy contacts engine — list/add/delete decoy contacts shown when the
//! duress PIN is entered. Plain ScreenModel renderer; mutations flow
//! through `AppEngine::intercept_decoy_contacts_action` so the frontend
//! stays a humble renderer (Phase 2c of
//! `2026-05-01-android-humble-ui-deep-retirement`).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Summary info for a decoy contact.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecoyContactItem {
    pub id: String,
    pub display_name: String,
}

/// Engine that displays + mutates the decoy contact list.
pub struct DecoyContactsEngine {
    decoys: Vec<DecoyContactItem>,
    new_decoy_name: String,
    pending_delete_id: Option<String>,
    locale: Locale,
}

impl DecoyContactsEngine {
    pub fn new(decoys: Vec<DecoyContactItem>) -> Self {
        Self {
            decoys,
            new_decoy_name: String::new(),
            pending_delete_id: None,
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

    pub fn decoys(&self) -> &[DecoyContactItem] {
        &self.decoys
    }

    pub fn new_decoy_name(&self) -> &str {
        &self.new_decoy_name
    }

    pub fn pending_delete_id(&self) -> Option<&str> {
        self.pending_delete_id.as_deref()
    }

    pub fn clear_pending_delete(&mut self) {
        self.pending_delete_id = None;
    }

    pub fn clear_new_decoy_name(&mut self) {
        self.new_decoy_name.clear();
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = vec![Component::InfoPanel {
            id: "decoy_info".into(),
            icon: Some("shield".into()),
            title: self.t("decoy_contacts.title"),
            items: vec![InfoItem {
                icon: Some("info".into()),
                title: self.t("decoy_contacts.what_are_they_label"),
                detail: self.t("decoy_contacts.explanation"),
            }],
            a11y: None,
        }];

        if self.decoys.is_empty() {
            components.push(Component::Text {
                id: "empty_state".into(),
                content: self.t("decoy_contacts.empty_state"),
                style: TextStyle::Body,
            });
        } else {
            let items: Vec<ActionListItem> = self
                .decoys
                .iter()
                .map(|d| ActionListItem {
                    id: d.id.clone(),
                    label: d.display_name.clone(),
                    icon: Some("person".into()),
                    detail: None,
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale,
                            "decoy_contacts.item_a11y",
                            &[("name", &d.display_name)],
                        )),
                        hint: Some(self.t("decoy_contacts.remove_hint")),
                        role: None,
                    }),
                    info_key: None,
                })
                .collect();
            components.push(Component::ActionList {
                id: "decoys".into(),
                items,
            });
        }

        if self.pending_delete_id.is_some() {
            components.push(Component::InlineConfirm {
                id: "delete_decoy".into(),
                warning: self.t("decoy_contacts.remove_warning"),
                confirm_text: self.t("decoy_contacts.remove_button"),
                cancel_text: self.t("action.cancel"),
                destructive: true,
                a11y: None,
            });
        }

        components.push(Component::TextInput {
            id: "new_decoy_name".into(),
            label: self.t("decoy_contacts.new_name_label"),
            value: self.new_decoy_name.clone(),
            placeholder: Some(self.t("decoy_contacts.new_name_placeholder")),
            max_length: Some(64),
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some(self.t("decoy_contacts.new_name_a11y")),
                hint: Some(self.t("decoy_contacts.new_name_hint")),
                role: None,
            }),
            info_key: None,
        });

        let add_enabled =
            !self.new_decoy_name.trim().is_empty() && self.pending_delete_id.is_none();

        ScreenModel {
            screen_id: "decoy_contacts".into(),
            title: self.t("decoy_contacts.title"),
            subtitle: None,
            components,
            actions: vec![ScreenAction {
                id: "add_decoy".into(),
                label: self.t("decoy_contacts.add_button"),
                style: ActionStyle::Primary,
                enabled: add_enabled,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DecoyContactsEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::DecoyContacts {
            new_name: self.new_decoy_name().to_string(),
            pending_delete_id: self.pending_delete_id().map(str::to_string),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "new_decoy_name" => {
                self.new_decoy_name = value;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "decoys" => {
                self.pending_delete_id = Some(item_id);
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                // The AppEngine intercept handles the actual side-effect (storage
                // write + cache invalidation) and returns UpdateScreen. If the
                // intercept doesn't fire (unexpected screen), we just refresh.
                "add_decoy" | "confirm_delete_decoy" => {
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "cancel_delete_decoy" => {
                    self.pending_delete_id = None;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: Tests access private DecoyContactsEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_decoys() -> Vec<DecoyContactItem> {
        vec![
            DecoyContactItem {
                id: "d1".into(),
                display_name: "Alice Example".into(),
            },
            DecoyContactItem {
                id: "d2".into(),
                display_name: "Bob Sample".into(),
            },
        ]
    }

    // @scenario: decoy_contacts :: User views decoy contact list
    #[test]
    fn empty_list_shows_empty_state_text() {
        let engine = DecoyContactsEngine::new(vec![]);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "decoy_contacts");
        assert_eq!(screen.title, "Decoy Contacts");
        let has_empty = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::Text { id, .. } if id == "empty_state"));
        assert!(has_empty, "expected empty_state Text component");
    }

    // @scenario: decoy_contacts :: User views decoy contact list
    #[test]
    fn populated_list_renders_action_list_with_each_decoy() {
        let engine = DecoyContactsEngine::new(sample_decoys());
        let screen = engine.current_screen();
        let action_list = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::ActionList { id, .. } if id == "decoys"))
            .expect("ActionList with id 'decoys' should be present");
        if let Component::ActionList { items, .. } = action_list {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].label, "Alice Example");
            assert_eq!(items[1].label, "Bob Sample");
        }
    }

    // @scenario: decoy_contacts :: User adds a decoy contact
    #[test]
    fn text_changed_updates_new_decoy_name_field() {
        let mut engine = DecoyContactsEngine::new(vec![]);
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "new_decoy_name".into(),
            value: "Charlie Demo".into(),
        });
        assert_eq!(engine.new_decoy_name(), "Charlie Demo");
    }

    // @scenario: decoy_contacts :: User adds a decoy contact
    #[test]
    fn add_decoy_action_enabled_only_with_non_empty_name() {
        let mut engine = DecoyContactsEngine::new(vec![]);
        let screen = engine.current_screen();
        let add_action = screen.actions.iter().find(|a| a.id == "add_decoy").unwrap();
        assert!(!add_action.enabled, "Add disabled when name is empty");

        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "new_decoy_name".into(),
            value: "Charlie".into(),
        });
        let screen = engine.current_screen();
        let add_action = screen.actions.iter().find(|a| a.id == "add_decoy").unwrap();
        assert!(add_action.enabled, "Add enabled when name is non-empty");
    }

    // @scenario: decoy_contacts :: User adds a decoy contact
    #[test]
    fn whitespace_only_name_does_not_enable_add() {
        let mut engine = DecoyContactsEngine::new(vec![]);
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "new_decoy_name".into(),
            value: "   ".into(),
        });
        let screen = engine.current_screen();
        let add_action = screen.actions.iter().find(|a| a.id == "add_decoy").unwrap();
        assert!(!add_action.enabled, "Add must reject whitespace-only names");
    }

    // @scenario: decoy_contacts :: User removes a decoy contact
    #[test]
    fn list_item_selection_marks_pending_delete() {
        let mut engine = DecoyContactsEngine::new(sample_decoys());
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "decoys".into(),
            item_id: "d1".into(),
        });
        assert_eq!(engine.pending_delete_id(), Some("d1"));
        let screen = engine.current_screen();
        let has_confirm = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "delete_decoy"));
        assert!(
            has_confirm,
            "expected InlineConfirm when pending_delete set"
        );
    }

    // @scenario: decoy_contacts :: User removes a decoy contact
    #[test]
    fn cancel_delete_clears_pending_state() {
        let mut engine = DecoyContactsEngine::new(sample_decoys());
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "decoys".into(),
            item_id: "d1".into(),
        });
        assert!(engine.pending_delete_id().is_some());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel_delete_decoy".into(),
        });
        assert_eq!(engine.pending_delete_id(), None);
    }

    // @scenario: decoy_contacts :: User removes a decoy contact
    #[test]
    fn add_disabled_while_pending_delete_to_avoid_clobber() {
        let mut engine = DecoyContactsEngine::new(sample_decoys());
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "new_decoy_name".into(),
            value: "Charlie".into(),
        });
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "decoys".into(),
            item_id: "d1".into(),
        });
        let screen = engine.current_screen();
        let add_action = screen.actions.iter().find(|a| a.id == "add_decoy").unwrap();
        assert!(
            !add_action.enabled,
            "Add must be disabled while delete-confirm is pending"
        );
    }
}

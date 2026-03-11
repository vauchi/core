// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact detail engine — view a single contact with a toggle between
//! "their info I can see" and "my info they can see".

use crate::ui::*;

/// Which perspective the user is viewing.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContactViewMode {
    /// Their shared fields (default — what they share with me).
    TheirInfo,
    /// My fields as visible to this contact (what I share with them).
    MyInfoForThem,
}

/// Data needed to show "my info they can see".
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SharedInfoView {
    /// The display name this contact sees (override or default).
    pub shared_display_name: String,
    /// My fields with visibility state for this contact.
    pub my_fields: Vec<FieldDisplay>,
    /// Group names that grant this contact visibility to my fields.
    pub visible_groups: Vec<String>,
}

/// Read-only engine that displays a single contact's details with a
/// perspective toggle.
#[derive(Clone, Debug)]
pub struct ContactDetailEngine {
    contact: ContactItem,
    fields: Vec<FieldDisplay>,
    shared_info: Option<SharedInfoView>,
    view_mode: ContactViewMode,
}

impl ContactDetailEngine {
    /// Create with only their info (no shared info available).
    pub fn new(contact: ContactItem, fields: Vec<FieldDisplay>) -> Self {
        Self {
            contact,
            fields,
            shared_info: None,
            view_mode: ContactViewMode::TheirInfo,
        }
    }

    /// Create with both perspectives available.
    pub fn with_shared_info(
        contact: ContactItem,
        fields: Vec<FieldDisplay>,
        shared_info: SharedInfoView,
    ) -> Self {
        Self {
            contact,
            fields,
            shared_info: Some(shared_info),
            view_mode: ContactViewMode::TheirInfo,
        }
    }

    /// Returns the current view mode.
    pub fn view_mode(&self) -> &ContactViewMode {
        &self.view_mode
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Mode toggle — only shown when shared info is available
        if self.shared_info.is_some() {
            components.push(Component::ToggleList {
                id: "view_mode".into(),
                label: "Perspective".into(),
                items: vec![
                    ToggleItem {
                        id: "their_info".into(),
                        label: "Their Info".into(),
                        selected: self.view_mode == ContactViewMode::TheirInfo,
                        subtitle: Some("What they share with me".into()),
                    },
                    ToggleItem {
                        id: "my_info_for_them".into(),
                        label: "My Info for Them".into(),
                        selected: self.view_mode == ContactViewMode::MyInfoForThem,
                        subtitle: Some("What I share with them".into()),
                    },
                ],
            });
        }

        match self.view_mode {
            ContactViewMode::TheirInfo => {
                components.push(Component::InfoPanel {
                    id: "contact_info".into(),
                    icon: None,
                    title: self.contact.name.clone(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Initials".into(),
                        detail: self.contact.avatar_initials.clone(),
                    }],
                });
                // Their fields — read-only, no visibility column
                components.push(Component::FieldList {
                    id: "fields".into(),
                    fields: self.fields.clone(),
                    visibility_mode: VisibilityMode::ReadOnly,
                    available_groups: vec![],
                });
            }
            ContactViewMode::MyInfoForThem => {
                if let Some(ref shared) = self.shared_info {
                    components.push(Component::InfoPanel {
                        id: "shared_name_info".into(),
                        icon: None,
                        title: "They see me as".into(),
                        items: vec![InfoItem {
                            icon: None,
                            title: "Display Name".into(),
                            detail: shared.shared_display_name.clone(),
                        }],
                    });
                    // My fields — show which groups grant visibility
                    components.push(Component::FieldList {
                        id: "my_fields".into(),
                        fields: shared.my_fields.clone(),
                        visibility_mode: VisibilityMode::PerGroup,
                        available_groups: shared.visible_groups.clone(),
                    });
                }
            }
        }

        let title = match self.view_mode {
            ContactViewMode::TheirInfo => self.contact.name.clone(),
            ContactViewMode::MyInfoForThem => {
                format!("Shared with {}", self.contact.name)
            }
        };

        ScreenModel {
            screen_id: "contact_detail".into(),
            title,
            subtitle: self.contact.subtitle.clone(),
            components,
            actions: vec![
                ScreenAction {
                    id: "edit".into(),
                    label: "Edit".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }
}

impl WorkflowEngine for ContactDetailEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // View mode toggle
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "view_mode" => {
                self.view_mode = match item_id.as_str() {
                    "their_info" => ContactViewMode::TheirInfo,
                    "my_info_for_them" => ContactViewMode::MyInfoForThem,
                    _ => return ActionResult::UpdateScreen(self.build_screen()),
                };
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                ActionResult::EditContact {
                    contact_id: self.contact.id.clone(),
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

/// Fallback engine for when a contact is not found.
#[derive(Clone, Debug)]
pub struct ContactNotFoundEngine {
    contact_id: String,
}

impl ContactNotFoundEngine {
    pub fn new(contact_id: String) -> Self {
        Self { contact_id }
    }
}

impl WorkflowEngine for ContactNotFoundEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_not_found".into(),
            title: "Contact Not Found".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "not_found".into(),
                icon: None,
                title: "Not Found".into(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Error".into(),
                    detail: format!("Contact '{}' was not found.", self.contact_id),
                }],
            }],
            actions: vec![ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            }],
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: Tests access private ContactViewMode and ContactDetailEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_contact() -> ContactItem {
        ContactItem {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("+41 79 123 45 67".into()),
            avatar_initials: "A".into(),
            status: None,
            searchable_fields: vec![],
        }
    }

    fn sample_fields() -> Vec<FieldDisplay> {
        vec![FieldDisplay {
            id: "f1".into(),
            field_type: "Phone".into(),
            label: "Mobile".into(),
            value: "+41 79 123 45 67".into(),
            visibility: UiFieldVisibility::Shown,
        }]
    }

    fn sample_shared_info() -> SharedInfoView {
        SharedInfoView {
            shared_display_name: "Bob (Work)".into(),
            my_fields: vec![
                FieldDisplay {
                    id: "mf1".into(),
                    field_type: "Email".into(),
                    label: "Work Email".into(),
                    value: "bob@work.com".into(),
                    visibility: UiFieldVisibility::Shown,
                },
                FieldDisplay {
                    id: "mf2".into(),
                    field_type: "Phone".into(),
                    label: "Personal".into(),
                    value: "+41 79 999 88 77".into(),
                    visibility: UiFieldVisibility::Hidden,
                },
            ],
            visible_groups: vec!["Work".into()],
        }
    }

    #[test]
    fn test_default_shows_their_info() {
        let engine = ContactDetailEngine::new(sample_contact(), sample_fields());
        let screen = engine.current_screen();

        assert_eq!(screen.screen_id, "contact_detail");
        assert_eq!(screen.title, "Alice");
        assert_eq!(engine.view_mode(), &ContactViewMode::TheirInfo);

        // No toggle when shared_info is None
        let has_toggle = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "view_mode"));
        assert!(!has_toggle, "Should not show toggle without shared info");
    }

    #[test]
    fn test_with_shared_info_shows_toggle() {
        let engine = ContactDetailEngine::with_shared_info(
            sample_contact(),
            sample_fields(),
            sample_shared_info(),
        );
        let screen = engine.current_screen();

        let has_toggle = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "view_mode"));
        assert!(
            has_toggle,
            "Should show toggle when shared info is available"
        );
    }

    #[test]
    fn test_toggle_to_my_info_shows_shared_name() {
        let mut engine = ContactDetailEngine::with_shared_info(
            sample_contact(),
            sample_fields(),
            sample_shared_info(),
        );

        // Switch to MyInfoForThem
        let result = engine.handle_action(UserAction::ItemToggled {
            component_id: "view_mode".into(),
            item_id: "my_info_for_them".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(engine.view_mode(), &ContactViewMode::MyInfoForThem);

        let screen = engine.current_screen();
        assert_eq!(screen.title, "Shared with Alice");

        // Should show shared display name
        let name_panel = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::InfoPanel { id, .. } if id == "shared_name_info"));
        assert!(name_panel.is_some(), "Should show shared name panel");
        if let Some(Component::InfoPanel { items, .. }) = name_panel {
            assert_eq!(items[0].detail, "Bob (Work)");
        }
    }

    #[test]
    fn test_toggle_to_my_info_shows_my_fields() {
        let mut engine = ContactDetailEngine::with_shared_info(
            sample_contact(),
            sample_fields(),
            sample_shared_info(),
        );

        engine.handle_action(UserAction::ItemToggled {
            component_id: "view_mode".into(),
            item_id: "my_info_for_them".into(),
        });

        let screen = engine.current_screen();
        let field_list = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::FieldList { id, .. } if id == "my_fields"));
        assert!(field_list.is_some(), "Should show my fields");
        if let Some(Component::FieldList { fields, .. }) = field_list {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].label, "Work Email");
            assert_eq!(fields[1].visibility, UiFieldVisibility::Hidden);
        }
    }

    #[test]
    fn test_toggle_back_to_their_info() {
        let mut engine = ContactDetailEngine::with_shared_info(
            sample_contact(),
            sample_fields(),
            sample_shared_info(),
        );

        // Switch to MyInfoForThem then back
        engine.handle_action(UserAction::ItemToggled {
            component_id: "view_mode".into(),
            item_id: "my_info_for_them".into(),
        });
        engine.handle_action(UserAction::ItemToggled {
            component_id: "view_mode".into(),
            item_id: "their_info".into(),
        });

        assert_eq!(engine.view_mode(), &ContactViewMode::TheirInfo);
        let screen = engine.current_screen();
        assert_eq!(screen.title, "Alice");
    }

    #[test]
    fn test_edit_action_still_works() {
        let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields());
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "edit".into(),
        });
        assert_eq!(
            result,
            ActionResult::EditContact {
                contact_id: "c1".into()
            }
        );
    }

    #[test]
    fn test_back_action_completes() {
        let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields());
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "back".into(),
        });
        assert_eq!(result, ActionResult::Complete);
    }
}

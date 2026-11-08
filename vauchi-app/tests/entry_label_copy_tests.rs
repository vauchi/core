// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::ui::{Component, FormDialogEngine, FormDialogType, UserAction, WorkflowEngine};

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn entry_label_explains_what_contacts_see() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    let screen = engine.current_screen();
    let label = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput { id, label, .. } if id == "field_label" => Some(label.as_str()),
            _ => None,
        });
    assert_eq!(label, Some("Entry label (optional)"));
    let hint = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "entry_label_hint" => {
                Some(content.as_str())
            }
            _ => None,
        });
    assert_eq!(hint, Some("Shown to your contacts beside this entry."));
}

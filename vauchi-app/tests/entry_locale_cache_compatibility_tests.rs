// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::i18n::load_locale_from_bytes;
use vauchi_app::ui::{Component, FormDialogEngine, FormDialogType, UserAction, WorkflowEngine};

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn old_cached_english_uses_new_bundled_keys_and_keeps_downloaded_copy() {
    let cached = br#"{"form.display_name_optional_label":"CDN entry label"}"#;
    load_locale_from_bytes("en", cached).expect("previously cached English locale");
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    let screen = engine.current_screen();
    let labels: Vec<_> = screen
        .components
        .iter()
        .filter_map(|component| match component {
            Component::TextInput { id, label, .. } => Some((id.as_str(), label.as_str())),
            _ => None,
        })
        .collect();
    assert!(
        labels.contains(&("field_value", "Email address")),
        "{labels:?}"
    );
    assert!(
        labels.contains(&("field_label", "CDN entry label")),
        "{labels:?}"
    );
    assert_eq!(screen.title, "Add Email");
}

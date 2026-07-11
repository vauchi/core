// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-6 (`2026-07-03-core-screens-bypass-i18n`): the avatar-editor
//! screen renders in the user's locale. Keys in `avatar.*`
//! (locales!103). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{AvatarEditorEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: avatar-editor :: source-picker screen renders in the active locale
// @internal
#[test]
fn avatar_editor_source_picker_renders_german() {
    load_german();
    let engine = AvatarEditorEngine::new("Alice".into(), false).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Avatar auswählen");
}

// @internal
#[test]
fn avatar_editor_source_picker_english_copy_unchanged() {
    let engine = AvatarEditorEngine::new("Alice".into(), false);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Choose Avatar");
}

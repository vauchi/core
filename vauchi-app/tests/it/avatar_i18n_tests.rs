// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-6 (`2026-07-03-core-screens-bypass-i18n`): the avatar-editor
//! screen renders in the user's locale. Keys in `avatar.*` (locales!103).
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{AvatarEditorEngine, WorkflowEngine};

fn picker_title(locale: Locale) -> String {
    AvatarEditorEngine::new("Alice".into(), false)
        .with_locale(locale)
        .current_screen()
        .title
        .clone()
}

// @scenario: avatar-editor :: source-picker screen renders in the active locale
// @internal
#[test]
fn avatar_editor_source_picker_renders_the_active_locale() {
    load_german();
    assert_translated(
        "source-picker title",
        &picker_title(Locale::German),
        &picker_title(Locale::English),
    );
}

// English is the source language and ships bundled, so pinning it here
// couples nothing external.
// @internal
#[test]
fn avatar_editor_source_picker_english_copy_unchanged() {
    let engine = AvatarEditorEngine::new("Alice".into(), false);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Choose Avatar");
}

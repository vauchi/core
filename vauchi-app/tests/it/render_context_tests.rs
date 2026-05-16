// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine::render_context` /
//! `set_render_context`.
//!
//! S2 of the settings-storage-by-sensitivity plan
//! (`_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`):
//! additive humble-surface accessor for the render-context tier
//! (Category 1 — locale + theme_id). This file pins the
//! AppEngine-side contract; the PAE JSON-shim contract is tested
//! in `core/vauchi-platform/tests/it/render_context_json_tests.rs`.

use vauchi_app::ui::{AppEngine, RenderContext};
use vauchi_core::api::Vauchi;

fn engine_no_identity() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn render_context_default_both_none() {
    let engine = engine_no_identity();
    let ctx = engine.render_context();
    assert_eq!(ctx.locale, None);
    assert_eq!(ctx.theme_id, None);
}

// @internal
#[test]
fn set_render_context_round_trips_both_fields() {
    let mut engine = engine_no_identity();
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: Some("cyber".to_string()),
    });
    let ctx = engine.render_context();
    assert_eq!(ctx.locale.as_deref(), Some("de"));
    assert_eq!(ctx.theme_id.as_deref(), Some("cyber"));
}

// @internal
#[test]
fn set_render_context_overwrites_previous() {
    let mut engine = engine_no_identity();
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: Some("cyber".to_string()),
    });
    engine.set_render_context(RenderContext {
        locale: Some("fr".to_string()),
        theme_id: Some("classic".to_string()),
    });
    let ctx = engine.render_context();
    assert_eq!(ctx.locale.as_deref(), Some("fr"));
    assert_eq!(ctx.theme_id.as_deref(), Some("classic"));
}

// @internal
#[test]
fn set_render_context_fields_independent() {
    // User picks an explicit locale but no theme.
    let mut engine = engine_no_identity();
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: None,
    });
    let ctx = engine.render_context();
    assert_eq!(ctx.locale.as_deref(), Some("de"));
    assert_eq!(ctx.theme_id, None);
}

// @internal
#[test]
fn render_context_is_storage_independent() {
    // Storage-only setting: frontends push render context before
    // identity exists (locale is applied at first paint). The
    // accessor must work pre-identity — same invariant the
    // retired `app_preferences_persist_without_identity` test
    // pinned for the legacy vault-backed path.
    let mut engine = engine_no_identity();
    assert!(!engine.vauchi().has_identity());
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: Some("cyber".to_string()),
    });
    let ctx = engine.render_context();
    assert_eq!(ctx.locale.as_deref(), Some("de"));
    assert_eq!(ctx.theme_id.as_deref(), Some("cyber"));
}

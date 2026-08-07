// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3b (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the GDPR
//! privacy overview, the identity-deletion review and the panic-shred
//! confirm render in the user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. Placeholder
//! interpolation is asserted separately, since that IS core's to hold.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{Component, DeletionSummary, GdprEngine, UserAction, WorkflowEngine};

fn engine_for(locale: Locale) -> GdprEngine {
    GdprEngine::new(None, "Active".into(), locale).with_deletion_summary(DeletionSummary {
        contact_count: 3,
        has_backup: false,
        device_count: 2,
    })
}

/// Copy a shell would show on the overview and the deletion review.
struct GdprCopy {
    overview_screen_id: String,
    overview_title: String,
    delete_action: String,
    export_action: String,
    review_title: String,
    review_subtitle: String,
    panel_title: String,
    identity_item: String,
    contact_count_item: String,
    irreversible_item: String,
    cancel_action: String,
}

fn walk_gdpr(locale: Locale) -> GdprCopy {
    let mut engine = engine_for(locale);

    let overview = engine.current_screen();
    let overview_screen_id = overview.screen_id.clone();
    let overview_title = overview.title.clone();
    let delete_action = action_label(&overview, "delete");
    let export_action = action_label(&overview, "export");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    let review = engine.current_screen();
    let review_title = review.title.clone();
    let review_subtitle = review.subtitle.clone().expect("review subtitle present");
    let Component::InfoPanel { title, items, .. } = &review.components[0] else {
        panic!("review screen leads with the deletion InfoPanel");
    };

    GdprCopy {
        overview_screen_id,
        overview_title,
        delete_action,
        export_action,
        review_title,
        review_subtitle,
        panel_title: title.clone(),
        identity_item: items[0].title.clone(),
        contact_count_item: items[1].title.clone(),
        irreversible_item: items
            .last()
            .expect("irreversible warning item")
            .title
            .clone(),
        cancel_action: action_label(&review, "cancel"),
    }
}

// @scenario: security :: identity deletion review renders in the active locale
// @internal
#[test]
fn gdpr_overview_and_delete_review_render_the_active_locale() {
    load_german();
    let de = walk_gdpr(Locale::German);
    let en = walk_gdpr(Locale::English);

    // Screen ids are identifiers, not copy — they must NOT translate.
    assert_eq!(de.overview_screen_id, "privacy_settings");
    assert_eq!(de.overview_screen_id, en.overview_screen_id);

    assert_translated("overview title", &de.overview_title, &en.overview_title);
    assert_translated("delete action", &de.delete_action, &en.delete_action);
    assert_translated("export action", &de.export_action, &en.export_action);
    assert_translated("review title", &de.review_title, &en.review_title);
    assert_translated("review subtitle", &de.review_subtitle, &en.review_subtitle);
    assert_translated("deletion panel title", &de.panel_title, &en.panel_title);
    assert_translated("identity item", &de.identity_item, &en.identity_item);
    assert_translated(
        "irreversible warning",
        &de.irreversible_item,
        &en.irreversible_item,
    );
    assert_translated("cancel action", &de.cancel_action, &en.cancel_action);

    // Interpolation IS core's to hold: `{count}` must resolve through
    // get_string_with_args in every locale, whatever the surrounding
    // wording says.
    assert!(
        de.contact_count_item.contains('3'),
        "German contact-count placeholder did not interpolate, got {:?}",
        de.contact_count_item
    );
    assert!(
        en.contact_count_item.contains('3'),
        "English contact-count placeholder did not interpolate, got {:?}",
        en.contact_count_item
    );
    assert_translated(
        "contact-count item",
        &de.contact_count_item,
        &en.contact_count_item,
    );
}

// @scenario: security :: panic shred confirm renders in the active locale
// @internal
#[test]
fn gdpr_panic_shred_confirm_renders_the_active_locale() {
    load_german();

    let confirm_copy = |locale: Locale| {
        let mut engine = engine_for(locale);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "panic_shred".into(),
        });
        let confirm = engine.current_screen();
        (
            confirm.title.clone(),
            confirm.subtitle.clone().expect("confirm subtitle present"),
            action_label(&confirm, "confirm_shred"),
            action_label(&confirm, "cancel"),
        )
    };

    let (de_title, de_subtitle, de_confirm, de_cancel) = confirm_copy(Locale::German);
    let (en_title, en_subtitle, en_confirm, en_cancel) = confirm_copy(Locale::English);

    assert_translated("panic-shred title", &de_title, &en_title);
    assert_translated("panic-shred subtitle", &de_subtitle, &en_subtitle);
    assert_translated("confirm-shred action", &de_confirm, &en_confirm);
    assert_translated("cancel action", &de_cancel, &en_cancel);
}

// English stays the canonical key values (two builder literals converge:
// Export My Data + the consent subtitle) — pinned so future edits go
// through the locale files, not the builder. English is the source
// language and ships bundled, so pinning it couples nothing external.
// @internal
#[test]
fn gdpr_english_copy_matches_canonical_keys() {
    let mut engine = GdprEngine::new(None, "Active".into(), Locale::English);

    let overview = engine.current_screen();
    assert_eq!(overview.title, "Privacy & Data");
    assert_eq!(action_label(&overview, "export"), "Export My Data");

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "consent_actions".into(),
        item_id: "manage_consent".into(),
    });
    let consent = engine.current_screen();
    assert_eq!(consent.title, "Manage Consent");
    assert_eq!(
        consent.subtitle.as_deref(),
        Some("Review and update data consent")
    );
}

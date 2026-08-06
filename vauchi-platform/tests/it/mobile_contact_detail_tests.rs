// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the G4 ContactDetail typed view-state surface.
//!
//! Verifies that `DomainCommand::ContactDetailViewState` returns the
//! canonical `actions`/`badges`/`banners` lists for various contact
//! states — closes the iOS/Android Verify-button divergence (audit V4)
//! and replaces frontend `if contact.isVerified | isRecoveryTrusted |
//! isHidden | reciprocity == ...` branches with a typed list.
//!
//! Slice 32g (2026-05-17) retired the
//! `VauchiPlatform::contact_detail_view_state` UniFFI export these
//! tests previously called; the dispatch path is the only public
//! entry point now.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileContactDetailAction, MobileContactDetailBadge,
    MobileContactDetailBanner, MobileContactDetailViewState, MobileError, PlatformAppEngine,
    PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    drive_onboarding(&engine);
    (engine, dir)
}

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input<'v>(nodes: &'v [serde_json::Value]) -> Option<&'v serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Alice"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

fn add_exchanged(engine: &PlatformAppEngine, name: &str, pk_seed: u8) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [pk_seed; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        // Non-zero timestamp so `added_time_display_string` formats a
        // human-readable string instead of a placeholder for epoch.
        1_700_000_000,
    );
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

fn add_imported(engine: &PlatformAppEngine, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact_id = format!("contact-{name}");
    let contact = vauchi_core::Contact::from_import(
        contact_id,
        card,
        vauchi_core::ImportSource::VcardFile,
        None,
        0,
    );
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

fn view_state(
    engine: &PlatformAppEngine,
    contact_id: String,
) -> Result<MobileContactDetailViewState, MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::ContactDetailViewState { contact_id })
        .map(|r| match r {
            DomainCommandResult::ContactDetailView { state } => state,
            other => panic!("expected ContactDetailView, got {other:?}"),
        })
}

fn has_action(actions: &[MobileContactDetailAction], target: &MobileContactDetailAction) -> bool {
    actions.iter().any(|a| a == target)
}

// @internal
#[test]
fn contact_detail_view_state_returns_error_when_contact_missing() {
    let (engine, _dir) = setup();
    let result = view_state(&engine, "nonexistent-id".to_string());
    assert!(
        result.is_err(),
        "missing contact must return an error, not a default view state"
    );
}

// @internal
#[test]
fn fresh_exchanged_contact_has_no_verified_badge_no_recovery_badge() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    assert!(
        !state.badges.contains(&MobileContactDetailBadge::Verified),
        "fresh exchanged contact must not show Verified badge"
    );
    assert!(
        !state
            .badges
            .contains(&MobileContactDetailBadge::RecoveryTrusted),
        "fresh exchanged contact must not show RecoveryTrusted badge"
    );
}

// @internal
#[test]
fn imported_contact_actions_include_delete_not_archive() {
    let (engine, _dir) = setup();
    let id = add_imported(&engine, "Eve");

    let state = view_state(&engine, id).unwrap();

    assert!(
        has_action(&state.actions, &MobileContactDetailAction::Delete),
        "imported contact must offer Delete action (hard-delete)"
    );
    assert!(
        !has_action(&state.actions, &MobileContactDetailAction::Archive),
        "imported contact must NOT offer Archive — that's for exchanged contacts only"
    );
}

// @internal
#[test]
fn exchanged_contact_actions_include_archive_not_delete() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    assert!(
        has_action(&state.actions, &MobileContactDetailAction::Archive),
        "exchanged contact must offer Archive action (soft-delete)"
    );
    assert!(
        !has_action(&state.actions, &MobileContactDetailAction::Delete),
        "exchanged contact must NOT offer Delete — that's for imported only"
    );
}

// @internal
#[test]
fn toggle_recovery_trust_carries_current_state_for_label_flip() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    let toggle = state
        .actions
        .iter()
        .find_map(|a| match a {
            MobileContactDetailAction::ToggleRecoveryTrust { currently_trusted } => {
                Some(*currently_trusted)
            }
            _ => None,
        })
        .expect("ToggleRecoveryTrust must be present");
    assert!(
        !toggle,
        "fresh contact must report currently_trusted=false so the button labels as 'Trust', not 'Untrust'"
    );
}

// @internal
#[test]
fn toggle_hidden_carries_current_state_for_label_flip() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    let toggle = state
        .actions
        .iter()
        .find_map(|a| match a {
            MobileContactDetailAction::ToggleHidden { currently_hidden } => Some(*currently_hidden),
            _ => None,
        })
        .expect("ToggleHidden must be present");
    assert!(
        !toggle,
        "fresh contact must report currently_hidden=false so the button labels as 'Hide', not 'Unhide'"
    );
}

// @internal
#[test]
fn preview_as_action_carries_contact_id() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id.clone()).unwrap();

    let preview_id = state
        .actions
        .iter()
        .find_map(|a| match a {
            MobileContactDetailAction::PreviewAs { contact_id } => Some(contact_id.clone()),
            _ => None,
        })
        .expect("PreviewAs action must be present");
    assert_eq!(
        preview_id, id,
        "PreviewAs must carry the contact_id, not the user's own id or empty string"
    );
}

// @internal
#[test]
fn standard_actions_back_edit_verify_fingerprint_always_present() {
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    assert!(has_action(&state.actions, &MobileContactDetailAction::Back));
    assert!(has_action(&state.actions, &MobileContactDetailAction::Edit));
    assert!(has_action(
        &state.actions,
        &MobileContactDetailAction::VerifyFingerprint
    ));
}

// @internal
#[test]
fn no_banner_for_pre_feature_contact_with_unknown_reciprocity() {
    let (engine, _dir) = setup();
    // Imported contacts have Reciprocity::Unknown by construction.
    let id = add_imported(&engine, "Eve");

    let state = view_state(&engine, id).unwrap();

    let has_reciprocity_banner = state.banners.iter().any(|b| {
        matches!(
            b,
            MobileContactDetailBanner::ReciprocityPending { .. }
                | MobileContactDetailBanner::ReciprocityUnreciprocated { .. }
        )
    });
    assert!(
        !has_reciprocity_banner,
        "Reciprocity::Unknown (pre-feature / imported) must surface no banner"
    );
}

// @internal
#[test]
fn imported_contact_has_no_added_time_display() {
    // Imported contacts have no exchange_timestamp, so the field is None.
    // Frontends render nothing rather than a misleading "0 years ago".
    let (engine, _dir) = setup();
    let id = add_imported(&engine, "Eve");

    let state = view_state(&engine, id).unwrap();

    assert!(
        state.added_time_display.is_none(),
        "imported contact must not surface added_time_display, got {:?}",
        state.added_time_display,
    );
}

// @internal
#[test]
fn exchanged_contact_surfaces_added_time_display_string() {
    // Exchanged contacts have a real exchange_timestamp so the formatter
    // produces a string. Without mocking SystemTime we cannot pin the
    // exact bucket — assert non-None and non-"Missing:" sentinel only.
    // Bucket-level coverage lives in the formatter's inline tests.
    let (engine, _dir) = setup();
    let id = add_exchanged(&engine, "Bob", 0x01);

    let state = view_state(&engine, id).unwrap();

    let display = state
        .added_time_display
        .as_deref()
        .expect("exchanged contact must surface added_time_display");
    assert!(
        !display.starts_with("Missing:"),
        "added_time_display must not surface the i18n Missing sentinel, got {display:?}",
    );
    assert!(
        !display.is_empty(),
        "added_time_display must not be an empty string",
    );
}

// ── G3 push-down: banner labels from the locale table + real-clock
//    reciprocity (the arm previously passed now=0, which disabled the
//    7-day unreciprocated timeout) ──

fn add_exchanged_with_reciprocity(
    engine: &PlatformAppEngine,
    name: &str,
    pk_seed: u8,
    exchange_ts: u64,
) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let mut contact = vauchi_core::Contact::from_exchange(
        [pk_seed; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        exchange_ts,
    );
    contact.set_reciprocity(vauchi_core::exchange::Reciprocity::Pending);
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

fn wall_clock_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// @internal
#[test]
fn fresh_pending_contact_shows_localized_pending_banner() {
    let (engine, _dir) = setup();
    let id = add_exchanged_with_reciprocity(&engine, "Bob", 0x21, wall_clock_now() - 60);

    let state = view_state(&engine, id).unwrap();

    match state.banners.as_slice() {
        [MobileContactDetailBanner::ReciprocityPending { label }] => {
            assert_eq!(label, "Waiting for them to share their info");
        }
        other => panic!("expected one pending banner, got {other:?}"),
    }
}

// @internal
#[test]
fn week_old_pending_contact_shows_localized_unreciprocated_banner() {
    let (engine, _dir) = setup();
    let eight_days = 8 * 24 * 60 * 60;
    let id = add_exchanged_with_reciprocity(&engine, "Bob", 0x22, wall_clock_now() - eight_days);

    let state = view_state(&engine, id).unwrap();

    match state.banners.as_slice() {
        [MobileContactDetailBanner::ReciprocityUnreciprocated { label }] => {
            assert_eq!(label, "They haven't shared their info");
        }
        other => panic!("expected one unreciprocated banner, got {other:?}"),
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The QR legs of the in-person hand-back, through the app engine
//! (vauchi/private#796, P33): who is offered it and what a scan may do.

use base64::Engine as _;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::AuthMode;
use vauchi_core::{Event, Vauchi};

use crate::common::lost_phone::{alice_pk_hex, lost};

const HAND_BACK: &str = "hand_back";

fn action_ids(engine: &AppEngine) -> Vec<String> {
    engine
        .current_screen()
        .contextual_actions
        .into_iter()
        .map(|action| action.id)
        .collect()
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn at_confirm_screen(guardian: Vauchi, claim: &str) -> AppEngine {
    let mut engine = AppEngine::new(guardian);
    engine.navigate_to(AppScreen::RecoveryHelp);
    let _ = press(&mut engine, "vouch");
    engine
        .dispatch(Event::QrScanned { data: claim.into() })
        .unwrap();
    engine
}

fn shows_a_qr(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::QrCode { .. }))
}

// @internal — DC-01: camera bytes are untrusted; a bad scan is refused and changes nothing.
#[test]
fn a_scan_that_is_no_share_is_refused_and_changes_nothing() {
    let owner = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&owner)).unwrap();
    let mut engine = AppEngine::new(phone);
    engine.initial_commands().unwrap();
    let base64 = base64::engine::general_purpose::STANDARD;
    let refusal = get_string(Locale::ENGLISH, "recovery.hand_back_share_refused");

    for scanned in [
        String::new(),
        "not base64 !!".to_owned(),
        "\u{0}\u{202e}".to_owned(),
        claim,
        base64.encode([0u8; 100]),
        "A".repeat(100_000),
    ] {
        let commands = engine.dispatch(Event::QrScanned { data: scanned }).unwrap();

        assert!(serde_json::to_string(&commands).unwrap().contains(&refusal));
        assert_eq!(engine.current_app_screen(), &AppScreen::Recovery);
        let status = engine.vauchi().hand_back_restore_status().unwrap();
        assert_eq!(status.unwrap().handed_back, 0);
        assert!(!engine.vauchi().has_identity());
    }
}

// @internal — P33: only a guardian keeping the owner's backup can hand one back.
#[test]
fn only_a_guardian_keeping_the_backup_is_offered_the_hand_back() {
    let mut owner = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&owner)).unwrap();

    let guardian = at_confirm_screen(owner.guardians.remove(0).into_vauchi(), &claim);
    assert_eq!(action_ids(&guardian).first().unwrap(), HAND_BACK);

    let contact = at_confirm_screen(owner.dave.into_vauchi(), &claim);
    assert!(!action_ids(&contact).contains(&HAND_BACK.to_owned()));
    assert!(action_ids(&contact).contains(&"create_voucher".to_owned()));
}

// @internal — ADR-032: a duress session looks like a guardian that keeps nothing.
#[test]
fn a_duress_session_is_not_offered_the_hand_back() {
    let mut owner = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&owner)).unwrap();
    let mut guardian = owner.guardians.remove(0).into_vauchi();
    guardian.setup_app_password("normal-pin").unwrap();
    guardian
        .setup_duress_pin_with_biometric_unlock("112233")
        .unwrap();
    assert_eq!(guardian.authenticate("112233").unwrap(), AuthMode::Duress);
    let parsed = guardian.parse_recovery_claim_b64(&claim).unwrap();

    assert!(!guardian.keeps_backup_of(&parsed).unwrap());
}

// @internal — P33: the hand-back asks for the session PIN first, as vouching does.
#[test]
fn the_hand_back_waits_for_the_session_pin() {
    let mut owner = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&owner)).unwrap();
    let mut guardian = owner.guardians.remove(0).into_vauchi();
    guardian.setup_app_password("app-password-123").unwrap();
    assert_eq!(
        guardian.authenticate("app-password-123").unwrap(),
        AuthMode::Normal
    );
    let mut engine = at_confirm_screen(guardian, &claim);

    let _ = press(&mut engine, HAND_BACK);

    assert!(!shows_a_qr(&engine), "no share before the PIN");
    assert!(!action_ids(&engine).contains(&HAND_BACK.to_owned()));
}

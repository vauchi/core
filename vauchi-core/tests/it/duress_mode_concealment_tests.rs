// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress mode conceals the duress setup (#462, ADR-032; owner decision
//! 2026-10-01): after the duress PIN unlocks the app, the duress and decoy
//! settings read as not set up, and anything the coercer does to them looks
//! like it worked for the session but never reaches the real configuration.
//! The covert alert and the biometric PIN prompt keep using the real one.

use super::common::helpers::create_vauchi_with_identity;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::types::DuressSettings;
use vauchi_core::{AuthMode, BiometricUnlockOutcome, Vauchi};

const APP_PIN: &str = "app-pin-1234";
const DURESS_PIN: &str = "135790";

fn real_settings() -> DuressSettings {
    DuressSettings {
        alert_contact_ids: vec!["contact-ally".into()],
        alert_message: "I need help".into(),
        include_location: false,
    }
}

/// Real duress setup, then unlocked with the duress PIN.
fn in_duress_mode() -> Vauchi {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PIN).unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    wb.save_duress_settings(&real_settings()).unwrap();
    wb.add_decoy_contact("decoy-dora", "Dora", &ContactCard::new("Dora"))
        .unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    wb
}

type Shown = Option<(Vec<String>, String, bool)>;

fn shown(settings: Option<DuressSettings>) -> Shown {
    settings.map(|s| (s.alert_contact_ids, s.alert_message, s.include_location))
}

fn decoy_names(wb: &Vauchi) -> Vec<String> {
    wb.list_decoy_contacts()
        .unwrap()
        .into_iter()
        .map(|(_, name, _)| name)
        .collect()
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
// @internal
#[test]
fn duress_mode_reads_the_duress_setup_as_not_set_up() {
    let wb = in_duress_mode();
    assert!(!wb.is_duress_enabled().unwrap());
    assert_eq!(shown(wb.load_duress_settings().unwrap()), None);
    assert!(decoy_names(&wb).is_empty());
}

// @scenario: duress_mode :: The duress setup cannot be changed in duress mode
// @internal
#[test]
fn duress_mode_writes_never_reach_the_real_setup() {
    let mut wb = in_duress_mode();

    wb.setup_duress_password("999999").unwrap();
    wb.save_duress_settings(&DuressSettings {
        alert_contact_ids: vec![],
        alert_message: "fake".into(),
        include_location: true,
    })
    .unwrap();
    wb.add_decoy_contact("decoy-x", "Xavier", &ContactCard::new("Xavier"))
        .unwrap();
    wb.remove_decoy_contact("decoy-dora").unwrap();
    wb.clear_decoy_contacts().unwrap();
    wb.delete_duress_settings().unwrap();
    wb.disable_duress().unwrap();

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert!(wb.is_duress_enabled().unwrap());
    assert_eq!(
        shown(wb.load_duress_settings().unwrap()),
        shown(Some(real_settings()))
    );
    assert_eq!(decoy_names(&wb), ["Dora"]);
    assert_eq!(
        wb.authenticate(DURESS_PIN).unwrap(),
        AuthMode::Duress,
        "the real duress PIN still works"
    );
    assert!(
        wb.authenticate("999999").is_err(),
        "a PIN set in duress mode was never stored"
    );
}

// @scenario: duress_mode :: Changes made in duress mode look like they worked
// @internal
#[test]
fn duress_mode_changes_look_like_they_worked_for_the_session() {
    let mut wb = in_duress_mode();
    let fake = DuressSettings {
        alert_contact_ids: vec![],
        alert_message: "fake".into(),
        include_location: true,
    };

    wb.setup_duress_password("999999").unwrap();
    wb.save_duress_settings(&fake).unwrap();
    wb.add_decoy_contact("decoy-x", "Xavier", &ContactCard::new("Xavier"))
        .unwrap();
    assert!(wb.is_duress_enabled().unwrap());
    assert_eq!(shown(wb.load_duress_settings().unwrap()), shown(Some(fake)));
    assert_eq!(decoy_names(&wb), ["Xavier"]);

    wb.remove_decoy_contact("decoy-x").unwrap();
    wb.disable_duress().unwrap();
    assert!(decoy_names(&wb).is_empty());
    assert!(!wb.is_duress_enabled().unwrap());
    assert_eq!(shown(wb.load_duress_settings().unwrap()), None);
}

// @scenario: duress_mode :: Biometric unlock still asks for the PIN
// @internal
#[test]
fn biometric_unlock_after_a_duress_session_still_asks_for_the_pin() {
    let mut wb = in_duress_mode();
    assert_eq!(
        wb.biometric_unlock_check().unwrap(),
        BiometricUnlockOutcome::PromptForDuressPin,
        "the real duress setup decides, not the session's concealed view"
    );
}

fn verified_contact(wb: &Vauchi, name: &str) -> String {
    let identity = vauchi_core::Identity::create(name, 0);
    let contact = vauchi_core::Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        vauchi_core::SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    wb.verify_contact_fingerprint(&id).unwrap();
    id
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
// The emergency snapshot is a second way to read the same setup (#462
// review): in duress mode it must not report duress protection or the
// real recovery-trusted contacts.
// @internal
#[test]
fn duress_mode_emergency_snapshot_reads_as_not_set_up() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PIN).unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    wb.save_duress_settings(&real_settings()).unwrap();
    let ally = verified_contact(&wb, "Ally");
    wb.toggle_recovery_trust(&ally).unwrap();

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    let real = wb.get_emergency_wipe_status().unwrap();
    assert!(real.duress_configured);
    assert_eq!(real.trusted_contact_count, 1);

    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    let shown = wb.get_emergency_wipe_status().unwrap();
    assert!(!shown.duress_configured);
    assert!(!shown.broadcast_configured);
    assert_eq!(shown.trusted_contact_count, 0);
    assert!(!shown.has_trusted_contacts);
}

// @scenario: duress_mode :: Changes made in duress mode look like they worked
// On a real install a decoy never shows among contacts; only duress mode
// shows decoys. A decoy added in duress mode therefore stays out of the
// contact list too — showing it would be the tell.
// @internal
#[test]
fn a_decoy_added_in_duress_mode_stays_out_of_the_contact_list() {
    let wb = in_duress_mode();
    wb.add_decoy_contact("decoy-x", "Xavier", &ContactCard::new("Xavier"))
        .unwrap();
    let names: Vec<String> = wb
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    assert_eq!(names, ["Dora"], "the stored decoys, as before the add");
}

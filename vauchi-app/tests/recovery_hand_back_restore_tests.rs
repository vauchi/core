// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::backup::{GuardianGroups, passphrase_layer};
use vauchi_core::sync::guardian_piece::{
    GuardianDelivery, HandBackShare, split_backup_for_hand_back, split_into_pieces,
};
use vauchi_core::{
    Command, Contact, ContactCard, Event, Identity, InputValue, SymmetricKey, Vauchi, VauchiConfig,
};

const PASSPHRASE: &str = "the owl flies at noon";

fn named(name: &str) -> Vauchi {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity(name).unwrap();
    phone
}

struct Saved {
    config: VauchiConfig,
    target: [u8; 32],
    temporary: [u8; 32],
    contact_id: String,
    contact_key: [u8; 32],
    owner: Vauchi,
    guardians: Vec<Vauchi>,
}

fn save_meeting(
    directory: &std::path::Path,
    shares: usize,
    partial_pieces: bool,
    passphrase: bool,
) -> Saved {
    let lost = named("Alice");
    let target = *lost.identity().unwrap().signing_public_key();
    let contact = Contact::from_exchange(
        *Identity::create("Dave", 0).signing_public_key(),
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_owned();
    let contact_key = *contact.shared_key().unwrap().as_bytes();
    lost.add_contact(contact).unwrap();
    let guardians: Vec<_> = (0..3).map(|i| named(&format!("G{i}"))).collect();
    let groups = GuardianGroups::new(
        guardians
            .iter()
            .map(|g| vec![*g.identity().unwrap().signing_public_key()])
            .collect(),
    )
    .unwrap();
    let (backup, sealed) = lost.export_guardian_backup_for_groups(&groups).unwrap();
    let mut backup = hex::decode(backup).unwrap();
    if passphrase {
        let salt = [9; 16];
        let key = passphrase_layer::passphrase_key(PASSPHRASE, &salt).unwrap();
        backup = passphrase_layer::seal(&key, &salt, &backup).unwrap();
    }
    if partial_pieces {
        backup = vec![7; 90_000];
    }
    let pieces = split_backup_for_hand_back(&backup).unwrap();
    let config = VauchiConfig::with_storage_path(directory.join("phone.db"))
        .with_storage_key(SymmetricKey::generate());
    let temporary = {
        let phone = Vauchi::new(config.clone()).unwrap();
        let claim = phone.begin_recovery(&hex::encode(target)).unwrap();
        let temporary = *phone
            .parse_recovery_claim_b64(&claim)
            .unwrap()
            .new_pk()
            .as_bytes();
        for (i, guardian) in guardians.iter().enumerate().take(shares) {
            let resealed = guardian
                .respond_to_recovery(&sealed[i][0], &temporary)
                .unwrap();
            let hand_back = HandBackShare::new(1, resealed, pieces[0].hash()).unwrap();
            assert_eq!(phone.take_hand_back(&hand_back.encode()).unwrap(), i + 1);
        }
        let count = if partial_pieces { 1 } else { pieces.len() };
        for piece in pieces.iter().take(count) {
            phone.take_hand_back_piece(&piece.encode()).unwrap();
        }
        temporary
    };
    Saved {
        config,
        target,
        temporary,
        contact_id,
        contact_key,
        owner: lost,
        guardians,
    }
}

fn labeled(value: &Value, label: &str, field: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if map.get("label").and_then(Value::as_str) == Some(label)
                && let Some(id) = map.get(field)
            {
                return Some(id.clone());
            }
            map.values().find_map(|v| labeled(v, label, field))
        }
        Value::Array(items) => items.iter().find_map(|v| labeled(v, label, field)),
        _ => None,
    }
}

fn activate(app: &mut AppEngine, commands: &[Command], key: &str) -> Vec<Command> {
    let surface_id = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            Command::PresentOverlay { surface_id, .. } => Some(surface_id.clone()),
            _ => None,
        })
        .unwrap();
    let id = labeled(
        &serde_json::to_value(commands).unwrap(),
        &get_string(Locale::ENGLISH, key),
        "interaction_id",
    )
    .unwrap_or_else(|| panic!("supplied action missing: {key}"));
    app.dispatch(Event::ActionActivated {
        surface_id,
        interaction_id: serde_json::from_value(id).unwrap(),
    })
    .unwrap()
}

fn enter_passphrase(app: &mut AppEngine, commands: &[Command], value: &str) -> Vec<Command> {
    let surface_id = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            _ => None,
        })
        .unwrap();
    let id = labeled(
        &serde_json::to_value(commands).unwrap(),
        &get_string(Locale::ENGLISH, "recovery.restore_passphrase_label"),
        "binding_id",
    )
    .unwrap();
    app.dispatch(Event::ValueChanged {
        surface_id,
        binding_id: serde_json::from_value(id).unwrap(),
        value: InputValue::Text(value.into()),
    })
    .unwrap()
}

fn assert_restored(app: &AppEngine, saved: &Saved) {
    assert_eq!(
        app.vauchi().identity().unwrap().signing_public_key(),
        &saved.target
    );
    let contact = app
        .vauchi()
        .get_contact(&saved.contact_id)
        .unwrap()
        .unwrap();
    assert_eq!(contact.display_name(), "Dave");
    assert_eq!(contact.shared_key().unwrap().as_bytes(), &saved.contact_key);
    assert_eq!(app.vauchi().pending_recovery_claim().unwrap(), None);
    assert_eq!(
        app.vauchi().rekey_pending_contacts().unwrap(),
        Vec::<String>::new()
    );
    assert!(
        !serde_json::to_string(&app.current_screen())
            .unwrap()
            .contains(&get_string(Locale::ENGLISH, "recovery.restore_done_detail"))
    );
}

// @internal — P32/P33: partial durable hand-backs keep the claim and cannot enter legacy voucher collection.
#[test]
fn identity_free_restore_waits_for_groups_and_complete_backup_after_restart() {
    for (shares, partial_pieces) in [(1, false), (3, true)] {
        let directory = tempfile::tempdir().unwrap();
        let saved = save_meeting(directory.path(), shares, partial_pieces, false);
        let mut app = AppEngine::new(Vauchi::new(saved.config.clone()).unwrap());
        let commands = app.initial_commands().unwrap();
        assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
        assert!(!app.vauchi().has_identity());
        let screen = app.current_screen();
        assert!(!screen.contextual_actions.iter().any(|action| action.label
            == get_string(Locale::ENGLISH, "recovery.restore_now")
            || action.label == get_string(Locale::ENGLISH, "recovery.add_voucher")));
        let claim = screen
            .components
            .iter()
            .find_map(|component| match component {
                Component::Text { id, content, .. } if id == "claim_data" => Some(content),
                _ => None,
            })
            .unwrap();
        let claim = app.vauchi().parse_recovery_claim_b64(claim).unwrap();
        assert_eq!(claim.old_pk().as_bytes(), &saved.target);
        assert_eq!(claim.new_pk().as_bytes(), &saved.temporary);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::ReplaceSurface { .. }))
        );
    }
}

// @internal — P28/P32: the supplied restore action recovers the old identity and relationships.
#[test]
fn identity_free_restore_uses_durable_hand_backs_and_finishes_on_my_card() {
    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 3, false, false);
    let mut app = AppEngine::new(Vauchi::new(saved.config.clone()).unwrap());
    let commands = app.initial_commands().unwrap();
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.restore_title")
    );
    let commands = activate(&mut app, &commands, "action_list.title");
    let commands = activate(&mut app, &commands, "action.cancel");
    assert!(!app.vauchi().has_identity());
    let commands = activate(&mut app, &commands, "recovery.restore_button");
    let commands = activate(&mut app, &commands, "recovery.restore_now");
    assert_restored(&app, &saved);
    activate(&mut app, &commands, "action.done");
    assert_eq!(app.current_app_screen(), &AppScreen::MyInfo);
    drop(app);
    let reopened = AppEngine::new(Vauchi::new(saved.config.clone()).unwrap());
    assert_restored(&reopened, &saved);
}

// @internal — P32/DC-02: refusal preserves the recovery, and a correct passphrase restores it.
#[test]
fn identity_free_restore_requests_passphrase_and_preserves_state_on_refusal() {
    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 3, false, true);
    let mut app = AppEngine::new(Vauchi::new(saved.config.clone()).unwrap());
    let commands = app.initial_commands().unwrap();
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.restore_title")
    );
    let commands = enter_passphrase(&mut app, &commands, "wrong passphrase");
    let commands = activate(&mut app, &commands, "recovery.restore_now");
    assert!(!app.vauchi().has_identity());
    assert!(app.current_screen().components.iter().any(|component| matches!(component,
        Component::Text { id, content, .. } if id == "restore_error" && content == &get_string(Locale::ENGLISH, "recovery.restore_refused"))));
    let commands = enter_passphrase(&mut app, &commands, PASSPHRASE);
    activate(&mut app, &commands, "recovery.restore_now");
    assert_restored(&app, &saved);
}

// @internal — DC-02: shares from separate ceremonies cannot satisfy one backup's threshold.
#[test]
fn identity_free_restore_never_combines_groups_from_different_ceremonies() {
    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 1, false, false);
    let groups = GuardianGroups::new(
        saved
            .guardians
            .iter()
            .map(|g| vec![*g.identity().unwrap().signing_public_key()])
            .collect(),
    )
    .unwrap();
    let (backup, sealed) = saved
        .owner
        .export_guardian_backup_for_groups(&groups)
        .unwrap();
    let pieces = split_backup_for_hand_back(&hex::decode(backup).unwrap()).unwrap();
    let phone = Vauchi::new(saved.config.clone()).unwrap();
    for (i, guardian) in saved.guardians.iter().enumerate().skip(1) {
        let resealed = guardian
            .respond_to_recovery(&sealed[i][0], &saved.temporary)
            .unwrap();
        let share = HandBackShare::new(2, resealed, pieces[0].hash()).unwrap();
        assert_eq!(phone.take_hand_back(&share.encode()).unwrap(), i + 1);
    }
    for piece in pieces {
        phone.take_hand_back_piece(&piece.encode()).unwrap();
    }
    let status = phone.hand_back_restore_status().unwrap().unwrap();
    assert_eq!(status.handed_back, 3);
    assert!(!status.can_attempt_restore);
    let mut app = AppEngine::new(phone);
    app.initial_commands().unwrap();
    assert!(!app.vauchi().has_identity());
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.claim_created_title")
    );
}

// @internal — P32: readiness is a read model and initialized identities stay on their separate path.
#[cfg(feature = "testing")]
#[test]
fn identity_free_restore_readiness_is_read_only_and_scoped_to_recovery() {
    let mut fresh = Vauchi::in_memory().unwrap();
    assert_eq!(fresh.hand_back_restore_status().unwrap(), None);
    fresh.begin_recovery(&"01".repeat(32)).unwrap();
    fresh.create_identity("Alice").unwrap();
    assert_eq!(fresh.hand_back_restore_status().unwrap(), None);

    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 3, false, false);
    let phone = Vauchi::new(saved.config.clone()).unwrap();
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();
    let status = phone.hand_back_restore_status().unwrap().unwrap();
    assert_eq!(status.handed_back, 3);
    assert!(status.can_attempt_restore);
    assert!(!status.needs_passphrase);
    assert!(!phone.has_identity());
}

// @internal — DC-02: an unreadable hand-back cannot disappear into a claim-only or ready state.
#[cfg(feature = "testing")]
#[test]
fn identity_free_restore_refuses_unreadable_stored_hand_backs() {
    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 3, false, false);
    let phone = Vauchi::new(saved.config.clone()).unwrap();
    phone
        .storage()
        .connection()
        .execute(
            "UPDATE recovery_hand_backs SET delivery_encrypted = X'00'",
            [],
        )
        .unwrap();
    let mut app = AppEngine::new(phone);
    app.initial_commands().unwrap();
    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    assert_eq!(
        app.current_screen().title,
        get_string(Locale::ENGLISH, "recovery.load_error")
    );
    assert!(!app.vauchi().has_identity());
}

// @internal — DC-02: a prepared same-identity restore cannot turn into a legacy import after identity changes.
#[test]
fn identity_free_restore_action_keeps_its_prepared_mode_when_an_identity_appears() {
    let directory = tempfile::tempdir().unwrap();
    let saved = save_meeting(directory.path(), 3, false, false);
    let mut app = AppEngine::new(Vauchi::new(saved.config.clone()).unwrap());
    let commands = app.initial_commands().unwrap();
    app.vauchi_mut()
        .create_identity("Unexpected identity")
        .unwrap();
    let unexpected = *app.vauchi().identity().unwrap().signing_public_key();
    let groups = GuardianGroups::new(
        saved
            .guardians
            .iter()
            .map(|g| vec![*g.identity().unwrap().signing_public_key()])
            .collect(),
    )
    .unwrap();
    let (backup, sealed) = saved
        .owner
        .export_guardian_backup_for_groups(&groups)
        .unwrap();
    let backup = hex::decode(backup).unwrap();
    for (i, guardian) in saved.guardians.iter().enumerate() {
        let contact = Contact::from_exchange(
            *guardian.identity().unwrap().signing_public_key(),
            ContactCard::new(&format!("G{i}")),
            SymmetricKey::generate(),
            0,
        );
        let id = contact.id().to_owned();
        app.vauchi().add_contact(contact).unwrap();
        let share = guardian
            .respond_to_recovery(&sealed[i][0], &unexpected)
            .unwrap();
        let delivery = GuardianDelivery::new(2, share, backup.clone()).unwrap();
        for piece in split_into_pieces([i as u8; 16], &delivery.encode()).unwrap() {
            app.vauchi()
                .storage()
                .returned_backups()
                .keep_piece(&id, &piece, 2)
                .unwrap();
        }
    }
    activate(&mut app, &commands, "recovery.restore_now");
    assert_eq!(
        app.vauchi().identity().unwrap().signing_public_key(),
        &unexpected
    );
    assert_eq!(
        app.vauchi()
            .get_contact(&saved.contact_id)
            .unwrap()
            .map(|contact| contact.display_name().to_owned()),
        None
    );
    assert!(app.current_screen().components.iter().any(|component| matches!(component,
        Component::Text { id, content, .. } if id == "restore_error" && content == &get_string(Locale::ENGLISH, "recovery.restore_refused"))));
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-person recovery ceremony through two real app engines, which
//! meet only through the generic commands and events shells carry
//! (vauchi/private#796, P32, P33, plan step 4). Domain APIs appear only to
//! arrange the owner's past and to read the outcome.

use serde_json::Value;
use sha2::{Digest, Sha256};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::HandBackRestoreStatus;
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::{Command, Event, SymmetricKey, Vauchi, VauchiConfig};

use crate::common::lost_phone::{Lost, alice_pk_hex, lost};

const SMALLEST_MTU: usize = 23;
const LINK_PEER: &str = "meeting-peer";

fn shown_qr(commands: &[Command]) -> Option<String> {
    fn walk(value: &Value) -> Option<String> {
        match value {
            Value::Object(fields) => {
                if let Some(qr) = fields.get("Qr")
                    && qr["purpose"] == "display"
                {
                    return qr["payloads"][0].as_str().map(str::to_owned);
                }
                fields.values().find_map(walk)
            }
            Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(&serde_json::to_value(commands).unwrap())
}

fn press_primary(engine: &mut AppEngine, commands: &[Command]) -> Vec<Command> {
    let (surface_id, interaction_id) = commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .primary
                .as_ref()
                .map(|action| (surface_id.clone(), action.interaction_id.clone())),
            _ => None,
        })
        .expect("the screen supplies a primary action");
    engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .unwrap()
}

fn scan(engine: &mut AppEngine, commands: &[Command], data: String) -> Vec<Command> {
    assert!(
        commands.contains(&Command::QrRequestScan),
        "the screen asks the hardware shell for a scan"
    );
    engine.dispatch(Event::QrScanned { data }).unwrap()
}

/// Alice's new phone in recovery mode, showing its claim.
struct RecoveringPhone {
    engine: AppEngine,
    commands: Vec<Command>,
    claim: String,
}

fn begin(owner: &Lost, phone: Vauchi) -> RecoveringPhone {
    begin_recovery_of(&alice_pk_hex(owner), phone)
}

fn begin_for(target: &[u8; 32], phone: Vauchi) -> RecoveringPhone {
    begin_recovery_of(&hex::encode(target), phone)
}

fn begin_recovery_of(target_hex: &str, phone: Vauchi) -> RecoveringPhone {
    let claim = phone.begin_recovery(target_hex).unwrap();
    let mut engine = AppEngine::new(phone);
    let commands = engine.initial_commands().unwrap();
    assert_eq!(engine.current_app_screen(), &AppScreen::Recovery);
    assert_eq!(
        shown_qr(&commands).as_deref(),
        Some(claim.as_str()),
        "the new phone shows its claim as a QR code"
    );
    RecoveringPhone {
        engine,
        commands,
        claim,
    }
}

/// The phone and one guardian, face to face, once the share has crossed.
struct Meeting<'phone> {
    phone: &'phone mut AppEngine,
    guardian: AppEngine,
    guardian_commands: Vec<Command>,
}

fn meet(phone: &mut RecoveringPhone, guardian: Vauchi) -> Meeting<'_> {
    let shares_before = restore_status(&phone.engine).handed_back;
    let mut guardian = AppEngine::new(guardian);
    guardian.navigate_to(AppScreen::RecoveryHelp);
    let opened = guardian.initial_commands().unwrap();
    let vouching = press_primary(&mut guardian, &opened);
    let scanning = press_primary(&mut guardian, &vouching);
    let recognized = scan(&mut guardian, &scanning, phone.claim.clone());
    let guardian_commands = press_primary(&mut guardian, &recognized);

    let share = shown_qr(&guardian_commands).expect("the guardian shows its share as a QR code");
    let scanning = press_primary(&mut phone.engine, &phone.commands);
    scan(&mut phone.engine, &scanning, share);
    phone.commands = phone.engine.initial_commands().unwrap();
    assert_eq!(
        restore_status(&phone.engine).handed_back,
        shares_before + 1,
        "the phone took the guardian's share"
    );
    Meeting {
        phone: &mut phone.engine,
        guardian,
        guardian_commands,
    }
}

fn restore_status(phone: &AppEngine) -> HandBackRestoreStatus {
    phone
        .vauchi()
        .hand_back_restore_status()
        .unwrap()
        .expect("a recovery is under way")
}

impl Meeting<'_> {
    /// Carries the guardian's GATT writes to the phone and its
    /// acknowledgements back, at most `limit` writes; returns the largest.
    fn carry_backup(&mut self, limit: usize) -> usize {
        let mut largest = 0;
        for _ in 0..limit {
            let Some(data) = take_write(&mut self.guardian_commands, CHAR_DATA_WRITE) else {
                break;
            };
            largest = largest.max(data.len());
            let mut acknowledgements = self
                .phone
                .dispatch(Event::BleCharacteristicRead {
                    device_id: LINK_PEER.into(),
                    direction: BleLinkDirection::Outbound,
                    uuid: CHAR_DATA_NOTIFY.into(),
                    data,
                })
                .unwrap();
            let Some(acknowledgement) = take_write(&mut acknowledgements, CHAR_DATA_NOTIFY) else {
                break;
            };
            self.guardian_commands = self
                .guardian
                .dispatch(Event::BleCharacteristicNotified {
                    device_id: LINK_PEER.into(),
                    direction: BleLinkDirection::Outbound,
                    uuid: CHAR_DATA_NOTIFY.into(),
                    data: acknowledgement,
                })
                .unwrap();
        }
        largest
    }
}

fn take_write(commands: &mut Vec<Command>, characteristic: &str) -> Option<Vec<u8>> {
    let index = commands.iter().position(|command| {
        matches!(command, Command::BleWriteCharacteristic { uuid, .. } if uuid == characteristic)
    })?;
    match commands.remove(index) {
        Command::BleWriteCharacteristic { data, .. } => Some(data),
        _ => None,
    }
}

fn kept_backup_hash(guardian: &Vauchi, target: &[u8; 32]) -> [u8; 32] {
    let kept = guardian
        .storage()
        .guardian_backups()
        .kept(&hex::encode(target))
        .unwrap()
        .expect("the guardian keeps the owner's backup");
    Sha256::digest(kept.backup()).into()
}

fn backup_is_whole(phone: &AppEngine, hash: &[u8; 32]) -> bool {
    phone
        .vauchi()
        .storage()
        .recovery()
        .backup_piece_progress(hash)
        .unwrap()
        .is_some_and(|(received, count)| received >= count)
}

fn own_key(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi.identity().unwrap().signing_public_key()
}

// @scenario: contact_recovery :: Social recovery restores the same identity on a new device
#[test]
#[ignore = "red until the ceremony is wired, vauchi/private#796 plan step 5"]
fn social_recovery_restores_the_same_identity_through_the_ceremony() {
    let owner = lost();
    let target = own_key(&owner.alice);
    let contacts = owner.alice.list_contacts().unwrap().len();
    let Lost { guardians, .. } = owner;

    let mut phone = begin_for(&target, Vauchi::in_memory().unwrap());
    for (index, guardian) in guardians.into_iter().enumerate() {
        let mut meeting = meet(&mut phone, guardian.into_vauchi());
        if index == 0 {
            meeting.carry_backup(usize::MAX);
        }
    }
    assert!(restore_status(&phone.engine).can_attempt_restore);
    let restoring = phone.engine.initial_commands().unwrap();
    press_primary(&mut phone.engine, &restoring);

    let restored = phone.engine.vauchi();
    assert_eq!(own_key(restored), target);
    assert_eq!(restored.list_contacts().unwrap().len(), contacts);
    assert_eq!(restored.pending_recovery_claim().unwrap(), None);
    assert_eq!(
        restored.rekey_pending_contacts().unwrap(),
        Vec::<String>::new()
    );
}

// @scenario: contact_recovery :: A recovery backup crosses a connection that accepts small parts
#[test]
#[ignore = "red until the ceremony is wired, vauchi/private#796 plan step 5"]
fn a_backup_crosses_a_link_at_the_smallest_mtu() {
    let mut owner = lost();
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);

    meeting.guardian_commands = meeting
        .guardian
        .dispatch(Event::BleMtuNegotiated {
            device_id: LINK_PEER.into(),
            mtu: SMALLEST_MTU as u32,
        })
        .unwrap();
    let largest_write = meeting.carry_backup(usize::MAX);

    assert!(largest_write > 0, "the backup travelled over the link");
    assert!(largest_write <= SMALLEST_MTU - 3);
    assert!(backup_is_whole(meeting.phone, &hash));
}

// @scenario: contact_recovery :: A busy recovery connection reports an incomplete transfer
#[test]
#[ignore = "red until the ceremony is wired, vauchi/private#796 plan step 5"]
fn a_busy_link_reports_an_incomplete_transfer_and_keeps_the_share() {
    let mut owner = lost();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    meeting.carry_backup(1);

    let reported = meeting
        .guardian
        .dispatch(Event::BleDisconnected {
            device_id: LINK_PEER.into(),
            direction: BleLinkDirection::Outbound,
            reason: "link busy".into(),
        })
        .unwrap();

    assert!(
        serde_json::to_string(&reported)
            .unwrap()
            .contains(r#""tone":"error""#),
        "the guardian is told the transfer did not finish"
    );
    let status = restore_status(meeting.phone);
    assert_eq!(status.handed_back, 1);
    assert!(!status.can_attempt_restore);
    assert!(!meeting.phone.vauchi().has_identity());
}

// @scenario: contact_recovery :: An interrupted backup transfer resumes after a restart
#[test]
#[ignore = "red until the ceremony is wired, vauchi/private#796 plan step 5"]
fn an_interrupted_transfer_resumes_after_a_restart() {
    let owner = lost();
    let target = own_key(&owner.alice);
    let Lost { guardians, .. } = owner;
    let mut guardians = guardians.into_iter().map(|guardian| guardian.into_vauchi());
    let first = guardians.next().unwrap();
    let hash = kept_backup_hash(&first, &target);
    let directory = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(directory.path().join("phone.db"))
        .with_storage_key(SymmetricKey::generate());

    let mut phone = begin_for(&target, Vauchi::new(config.clone()).unwrap());
    meet(&mut phone, first).carry_backup(1);
    drop(phone);

    let reopened = Vauchi::new(config).unwrap();
    let mut phone = begin_for(&target, reopened);
    assert_eq!(restore_status(&phone.engine).handed_back, 1);
    assert!(!backup_is_whole(&phone.engine, &hash));
    meet(&mut phone, guardians.next().unwrap()).carry_backup(usize::MAX);
    meet(&mut phone, guardians.next().unwrap());
    assert!(restore_status(&phone.engine).can_attempt_restore);
}

// @scenario: contact_recovery :: Cancelling a backup transfer ignores late parts
#[test]
#[ignore = "red until the ceremony is wired, vauchi/private#796 plan step 5"]
fn a_cancelled_transfer_stops_and_ignores_late_parts() {
    let mut owner = lost();
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    let late_part = take_write(&mut meeting.guardian_commands, CHAR_DATA_WRITE)
        .expect("the guardian began sending");

    let showing = meeting.phone.initial_commands().unwrap();
    let cancelled = press_primary(meeting.phone, &showing);
    assert!(
        cancelled
            .iter()
            .any(|command| matches!(command, Command::BleDisconnect { .. })),
        "cancelling stops the Bluetooth transfer"
    );

    let after_late_part = meeting
        .phone
        .dispatch(Event::BleCharacteristicRead {
            device_id: LINK_PEER.into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: late_part,
        })
        .unwrap();
    assert_eq!(after_late_part, Vec::<Command>::new());
    assert!(!backup_is_whole(meeting.phone, &hash));
}

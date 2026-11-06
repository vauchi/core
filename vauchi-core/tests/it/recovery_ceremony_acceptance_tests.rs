// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in-person recovery ceremony through two real app engines, which
//! meet only through the generic commands and events shells carry
//! (vauchi/private#796, P32, P33, plan step 4). Domain APIs appear only to
//! arrange the owner's past and to read the outcome.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use sha2::{Digest, Sha256};
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppPresentationError, AppScreen};
use vauchi_core::api::HandBackRestoreStatus;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::{Command, Event, SymmetricKey, Vauchi, VauchiConfig};

use crate::common::lost_phone::{
    Lost, alice_pk_hex, lost, lost_with_extra_contacts, lost_with_guardians_on,
};

#[path = "recovery_ceremony_fixture_tests.rs"]
mod fixture;

const SMALLEST_MTU: usize = 23;
const USUAL_MTU: u32 = 185;
const GUARDIAN_LINK: &str = "guardian-link";
// What `linux-hw` reports for a peripheral: a link event has no id, while a
// write names the central that made it.
const PHONE_LINK: &str = "";
const PHONE_ADDRESS: &str = "AA:BB:CC:DD:EE:FF";

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

fn press_primary(engine: &mut Actor, commands: &[Command]) -> Vec<Command> {
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

fn scan(engine: &mut Actor, commands: &[Command], data: String) -> Vec<Command> {
    assert!(
        commands.contains(&Command::QrRequestScan),
        "the screen asks the hardware shell for a scan"
    );
    engine.dispatch(Event::QrScanned { data }).unwrap()
}

/// An app engine whose traffic the fixture can record. `dispatch` and
/// `initial_commands` shadow the engine's, so scenarios read as they would
/// without recording.
struct Actor {
    name: &'static str,
    engine: AppEngine,
}

impl Actor {
    fn new(name: &'static str, vauchi: Vauchi) -> Self {
        Self {
            name,
            engine: AppEngine::new(vauchi),
        }
    }

    fn dispatch(&mut self, event: Event) -> Result<Vec<Command>, AppPresentationError> {
        let commands = self.engine.dispatch(event.clone())?;
        fixture::record(self.name, Some(&event), &commands);
        Ok(commands)
    }

    fn initial_commands(&mut self) -> Result<Vec<Command>, AppPresentationError> {
        let commands = self.engine.initial_commands()?;
        fixture::record(self.name, None, &commands);
        Ok(commands)
    }
}

impl std::ops::Deref for Actor {
    type Target = AppEngine;

    fn deref(&self) -> &AppEngine {
        &self.engine
    }
}

impl std::ops::DerefMut for Actor {
    fn deref_mut(&mut self) -> &mut AppEngine {
        &mut self.engine
    }
}

/// Alice's new phone in recovery mode, showing its claim.
struct RecoveringPhone {
    engine: Actor,
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
    fixture::register_secret(&claim, "RECOVERY-CLAIM");
    fixture::register_key(target_hex, "OLD-KEY");
    let temporary = phone.parse_recovery_claim_b64(&claim).unwrap();
    fixture::register_key(&hex::encode(temporary.new_pk().as_bytes()), "TEMPORARY-KEY");
    let mut engine = Actor::new("phone", phone);
    let commands = engine.initial_commands().unwrap();
    assert_eq!(engine.current_app_screen(), &AppScreen::Recovery);
    assert_eq!(
        shown_qr(&commands).as_deref(),
        Some(claim.as_str()),
        "the new phone shows its claim as a QR code"
    );
    RecoveringPhone { engine, claim }
}

/// The phone and one guardian, face to face, once the share has crossed.
struct Meeting<'phone> {
    phone: &'phone mut Actor,
    guardian: Actor,
    guardian_commands: Vec<Command>,
}

fn meet(phone: &mut RecoveringPhone, guardian: Vauchi) -> Meeting<'_> {
    let shares_before = restore_status(&phone.engine).handed_back;
    let mut guardian = Actor::new("guardian", guardian);
    guardian.navigate_to(AppScreen::RecoveryHelp);
    let opened = guardian.initial_commands().unwrap();
    let vouching = press_primary(&mut guardian, &opened);
    let scanning = press_primary(&mut guardian, &vouching);
    let recognized = scan(&mut guardian, &scanning, phone.claim.clone());
    let guardian_commands = press_primary(&mut guardian, &recognized);

    let share = shown_qr(&guardian_commands).expect("the guardian shows its share as a QR code");
    fixture::register_secret(&share, "HAND-BACK-SHARE-AND-TOKEN");
    let showing = phone.engine.initial_commands().unwrap();
    let scanning = press_primary(&mut phone.engine, &showing);
    scan(&mut phone.engine, &scanning, share);
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
    /// The phone finds the guardian by the token in its QR and dials it; the
    /// guardian starts sending once the link is ready.
    fn connect(&mut self, mtu: u32) {
        let advertised = self
            .guardian_commands
            .iter()
            .find_map(|command| match command {
                Command::BleStartAdvertising { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .expect("the guardian advertises for the meeting");
        let dialling = self
            .phone
            .dispatch(Event::BleDeviceDiscovered {
                id: GUARDIAN_LINK.into(),
                rssi: -40,
                adv_data: advertised[..2].to_vec(),
            })
            .unwrap();
        assert!(
            dialling.contains(&Command::BleConnect {
                device_id: GUARDIAN_LINK.into()
            }),
            "the phone dials the guardian whose token it scanned"
        );
        self.phone
            .dispatch(Event::BleConnected {
                device_id: GUARDIAN_LINK.into(),
                direction: BleLinkDirection::Outbound,
            })
            .unwrap();
        for event in [
            Event::BleConnected {
                device_id: PHONE_LINK.into(),
                direction: BleLinkDirection::Inbound,
            },
            Event::BleMtuNegotiated {
                device_id: PHONE_LINK.into(),
                mtu,
            },
            Event::BleGattReady {
                device_id: PHONE_LINK.into(),
            },
        ] {
            self.guardian_commands = self.guardian.dispatch(event).unwrap();
            assert!(
                !self
                    .guardian_commands
                    .contains(&Command::BleStopAdvertising),
                "on Linux this releases the GATT server the transfer needs"
            );
        }
    }

    /// Carries the guardian's notifications to the phone and its
    /// acknowledgements back, at most `limit` packets; returns the largest.
    fn carry_backup(&mut self, limit: usize) -> usize {
        let mut largest = 0;
        let _elision = fixture::Elision::begin();
        for packet in 0..limit {
            fixture::note_packet(packet);
            let Some(data) = take_write(&mut self.guardian_commands, CHAR_DATA_NOTIFY) else {
                break;
            };
            largest = largest.max(data.len());
            let mut acknowledgements = self
                .phone
                .dispatch(Event::BleCharacteristicNotified {
                    device_id: GUARDIAN_LINK.into(),
                    direction: BleLinkDirection::Outbound,
                    uuid: CHAR_DATA_NOTIFY.into(),
                    data,
                })
                .unwrap();
            let Some(acknowledgement) = take_write(&mut acknowledgements, CHAR_DATA_WRITE) else {
                break;
            };
            self.guardian_commands = self
                .guardian
                .dispatch(Event::BleCharacteristicNotified {
                    device_id: PHONE_ADDRESS.into(),
                    direction: BleLinkDirection::Inbound,
                    uuid: CHAR_DATA_WRITE.into(),
                    data: acknowledgement,
                })
                .unwrap();
            let drops_the_link = self
                .guardian_commands
                .iter()
                .any(|command| matches!(command, Command::BleDisconnect { .. }));
            let keeps_sending = self
                .guardian_commands
                .iter()
                .any(|command| matches!(command, Command::BleWriteCharacteristic { .. }));
            assert!(
                !(drops_the_link && keeps_sending),
                "the link stays up while more of the backup follows"
            );
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
fn social_recovery_restores_the_same_identity_through_the_ceremony() {
    let owner = lost();
    let target = own_key(&owner.alice);
    let contacts = owner.alice.list_contacts().unwrap().len();
    let Lost { guardians, .. } = owner;

    let mut phone = begin_for(&target, Vauchi::in_memory().unwrap());
    for (index, guardian) in guardians.into_iter().enumerate() {
        let mut meeting = meet(&mut phone, guardian.into_vauchi());
        if index == 0 {
            meeting.connect(USUAL_MTU);
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
fn a_backup_crosses_a_link_at_the_smallest_mtu() {
    let mut owner = lost();
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);

    meeting.connect(SMALLEST_MTU as u32);
    let largest_write = meeting.carry_backup(usize::MAX);

    assert!(largest_write > 0, "the backup travelled over the link");
    assert!(largest_write <= SMALLEST_MTU - 3);
    assert!(backup_is_whole(meeting.phone, &hash));
}

// @scenario: contact_recovery :: A busy recovery connection reports an incomplete transfer
#[test]
fn a_busy_link_reports_an_incomplete_transfer_and_keeps_the_share() {
    let mut owner = lost();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    meeting.connect(USUAL_MTU);
    meeting.carry_backup(1);

    let reported = meeting
        .guardian
        .dispatch(Event::BleDisconnected {
            device_id: PHONE_LINK.into(),
            direction: BleLinkDirection::Inbound,
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
    let mut meeting = meet(&mut phone, first);
    meeting.connect(USUAL_MTU);
    meeting.carry_backup(1);
    drop(meeting);
    drop(phone);

    let reopened = Vauchi::new(config).unwrap();
    let mut phone = begin_for(&target, reopened);
    assert_eq!(restore_status(&phone.engine).handed_back, 1);
    assert!(!backup_is_whole(&phone.engine, &hash));
    let mut meeting = meet(&mut phone, guardians.next().unwrap());
    meeting.connect(USUAL_MTU);
    meeting.carry_backup(usize::MAX);
    drop(meeting);
    meet(&mut phone, guardians.next().unwrap());
    assert!(restore_status(&phone.engine).can_attempt_restore);
}

// @scenario: contact_recovery :: Cancelling a backup transfer ignores late parts
#[test]
fn a_cancelled_transfer_stops_and_ignores_late_parts() {
    let mut owner = lost();
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    meeting.connect(USUAL_MTU);
    let late_part = take_write(&mut meeting.guardian_commands, CHAR_DATA_NOTIFY)
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
        .dispatch(Event::BleCharacteristicNotified {
            device_id: GUARDIAN_LINK.into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: late_part,
        })
        .unwrap();
    assert!(
        !after_late_part
            .iter()
            .any(|command| matches!(command, Command::BleWriteCharacteristic { .. })),
        "a cancelled transfer acknowledges nothing"
    );
    assert!(!backup_is_whole(meeting.phone, &hash));
}

// @internal — P33: a backup of several pieces crosses one link, which stays up between pieces.
#[test]
fn a_backup_of_several_pieces_crosses_one_link() {
    let mut owner = lost_with_extra_contacts(400);
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let kept = guardian
        .storage()
        .guardian_backups()
        .kept(&hex::encode(target))
        .unwrap()
        .unwrap();
    assert!(
        kept.backup().len() > 48 * 1024,
        "the backup must outgrow one piece, got {} bytes",
        kept.backup().len()
    );
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    meeting.connect(517);

    meeting.carry_backup(usize::MAX);

    assert!(backup_is_whole(meeting.phone, &hash));
    assert!(
        meeting
            .guardian_commands
            .contains(&Command::BleStopAdvertising),
        "the finished guardian withdraws its GATT server"
    );
}

// @internal — P33: a peripheral learns its MTU only from a write, so it applies to the next piece.
#[test]
fn an_mtu_learned_during_a_piece_serves_the_next_piece() {
    let mut owner = lost_with_extra_contacts(400);
    let target = own_key(&owner.alice);
    let guardian = owner.guardians.remove(0).into_vauchi();
    let hash = kept_backup_hash(&guardian, &target);
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let mut meeting = meet(&mut phone, guardian);
    meeting.connect(SMALLEST_MTU as u32);
    let first_piece_write = meeting.carry_backup(1);
    assert!(first_piece_write <= SMALLEST_MTU - 3);

    // The reply may hold the next write, so keep it across the report.
    let held = std::mem::take(&mut meeting.guardian_commands);
    meeting
        .guardian
        .dispatch(Event::BleMtuNegotiated {
            device_id: PHONE_ADDRESS.into(),
            mtu: 517,
        })
        .unwrap();
    meeting.guardian_commands = held;
    let largest_write = meeting.carry_backup(usize::MAX);

    assert!(backup_is_whole(meeting.phone, &hash));
    assert!(
        largest_write > SMALLEST_MTU - 3,
        "later pieces use the larger MTU, largest write was {largest_write}"
    );
}

// @internal — P33: the phone dials only the advertiser matching the token it scanned.
#[test]
fn the_phone_ignores_an_advertiser_with_another_token() {
    let mut owner = lost();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    let meeting = meet(&mut phone, guardian);
    let advertised = meeting
        .guardian_commands
        .iter()
        .find_map(|command| match command {
            Command::BleStartAdvertising { payload, .. } => Some(payload.clone()),
            _ => None,
        })
        .unwrap();
    let wrong = vec![!advertised[0], !advertised[1]];

    let commands = meeting
        .phone
        .dispatch(Event::BleDeviceDiscovered {
            id: "bystander".into(),
            rssi: -40,
            adv_data: wrong,
        })
        .unwrap();

    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::BleConnect { .. }))
    );
}

fn guardian_showing_its_share(guardian: Vauchi, claim: String) -> (Actor, Vec<Command>) {
    let mut engine = Actor::new("guardian", guardian);
    engine.navigate_to(AppScreen::RecoveryHelp);
    let opened = engine.initial_commands().unwrap();
    let vouching = press_primary(&mut engine, &opened);
    let scanning = press_primary(&mut engine, &vouching);
    let recognized = scan(&mut engine, &scanning, claim);
    let showing = press_primary(&mut engine, &recognized);
    (engine, showing)
}

fn clock_now() -> (Arc<FakeClock>, Arc<dyn Clock>) {
    let fake = Arc::new(FakeClock::new(SystemTime::now()));
    let shared: Arc<dyn Clock> = fake.clone();
    (fake, shared)
}

// @internal — P33: BleGattReady is optional, so a platform without it still starts after the grace.
#[test]
fn a_link_that_never_reports_ready_starts_after_the_grace() {
    let (clock, shared) = clock_now();
    let mut owner = lost_with_guardians_on(&shared);
    let claim = Vauchi::in_memory()
        .unwrap()
        .begin_recovery(&alice_pk_hex(&owner))
        .unwrap();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let (mut engine, _) = guardian_showing_its_share(guardian, claim);
    engine
        .dispatch(Event::BleConnected {
            device_id: PHONE_LINK.into(),
            direction: BleLinkDirection::Inbound,
        })
        .unwrap();
    let is_write = |command: &Command| matches!(command, Command::BleWriteCharacteristic { .. });

    let before_the_grace = engine.dispatch(Event::WakeupFired).unwrap();
    assert!(!before_the_grace.iter().any(is_write));

    clock.advance(Duration::from_secs(2));
    let after_the_grace = engine.dispatch(Event::WakeupFired).unwrap();
    assert!(after_the_grace.iter().any(is_write));
}

// @internal — DC-02: nobody connecting must not leave the guardian advertising forever.
#[test]
fn a_guardian_nobody_connects_to_stops_advertising() {
    let (clock, shared) = clock_now();
    let mut owner = lost_with_guardians_on(&shared);
    let claim = Vauchi::in_memory()
        .unwrap()
        .begin_recovery(&alice_pk_hex(&owner))
        .unwrap();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let (mut engine, showing) = guardian_showing_its_share(guardian, claim);
    assert!(
        showing
            .iter()
            .any(|command| matches!(command, Command::ScheduleWakeup { .. }))
    );

    assert!(
        !engine
            .dispatch(Event::WakeupFired)
            .unwrap()
            .contains(&Command::BleStopAdvertising)
    );
    clock.advance(Duration::from_secs(120));
    assert!(
        engine
            .dispatch(Event::WakeupFired)
            .unwrap()
            .contains(&Command::BleStopAdvertising)
    );
}

// @internal — DC-02: a phone finding nobody stops scanning, says so and keeps the share.
#[test]
fn a_phone_that_finds_nobody_stops_scanning_and_keeps_the_share() {
    let (clock, shared) = clock_now();
    let mut owner = lost();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let phone = Vauchi::in_memory_with_clock(shared).unwrap();
    let mut phone = begin(&owner, phone);
    meet(&mut phone, guardian);

    clock.advance(Duration::from_secs(120));
    let commands = phone.engine.dispatch(Event::WakeupFired).unwrap();

    assert!(commands.contains(&Command::BleStopScanning));
    assert_eq!(restore_status(&phone.engine).handed_back, 1);
    let shown = serde_json::to_string(&commands).unwrap();
    assert!(shown.contains(&get_string(Locale::ENGLISH, "recovery.backup_failed")));
}

// @internal — P33: leaving the screen ends the link, and coming back shows no stale share.
#[test]
fn leaving_the_hand_back_stops_advertising_and_forgets_the_share() {
    let mut owner = lost();
    let claim = Vauchi::in_memory()
        .unwrap()
        .begin_recovery(&alice_pk_hex(&owner))
        .unwrap();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let (mut engine, showing) = guardian_showing_its_share(guardian, claim);
    assert!(shown_qr(&showing).is_some());

    engine.navigate_to(AppScreen::Settings);
    let after_leaving = engine.dispatch(Event::WakeupFired).unwrap();
    assert!(after_leaving.contains(&Command::BleStopAdvertising));

    engine.navigate_to(AppScreen::RecoveryHelp);
    let back = engine.initial_commands().unwrap();
    assert_eq!(shown_qr(&back), None, "no share is shown after coming back");
}

// @internal — P33: a phone leaving the recovery screen stops looking for the guardian.
#[test]
fn a_phone_leaving_recovery_stops_scanning() {
    let mut owner = lost();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let mut phone = begin(&owner, Vauchi::in_memory().unwrap());
    meet(&mut phone, guardian);

    phone.engine.navigate_to(AppScreen::Help);
    let after_leaving = phone.engine.dispatch(Event::WakeupFired).unwrap();

    assert!(after_leaving.contains(&Command::BleStopScanning));
}

// @internal — P33: the back button leaves the screen as any other navigation does.
#[test]
fn going_back_from_the_hand_back_stops_advertising() {
    let mut owner = lost();
    let claim = Vauchi::in_memory()
        .unwrap()
        .begin_recovery(&alice_pk_hex(&owner))
        .unwrap();
    let guardian = owner.guardians.remove(0).into_vauchi();
    let (mut engine, _) = guardian_showing_its_share(guardian, claim);

    engine.navigate_back();
    let after_leaving = engine.dispatch(Event::WakeupFired).unwrap();

    assert!(after_leaving.contains(&Command::BleStopAdvertising));
}

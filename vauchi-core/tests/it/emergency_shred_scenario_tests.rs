// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The shred phases as a user lives them: a soft shred waits out its week,
//! a hard shred tells everyone while it still can and then destroys
//! everything, and a panic shred without network still leaves nothing.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::api::{
    DeletionError, DeletionManager, PreSignedPurgeRequest, PurgeSender, RevocationSender,
    ShredError, ShredManager,
};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::storage::{DeletionState, MemoryKeyStorage, SecureStorage, Storage};

const SMK: &str = "smk";
const START: u64 = 1_700_000_000;
const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

struct Device {
    dir: tempfile::TempDir,
    clock: Arc<FakeClock>,
    storage: Storage,
    keys: MemoryKeyStorage,
    me: Identity,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(START)));
    let storage = Storage::open(&dir.path().join("vauchi.db"), SymmetricKey::generate())
        .unwrap()
        .with_clock(clock.clone());
    let keys = MemoryKeyStorage::new();
    keys.save_key(
        SMK,
        ShreddingMasterKey::derive_from_seed(&[0x42; 32]).as_bytes(),
    )
    .unwrap();
    std::fs::write(dir.path().join("identity.json"), b"identity backup").unwrap();
    for name in ["Alice", "Bob"] {
        let peer = Identity::create(name, 0);
        storage
            .contacts()
            .save_contact(&Contact::from_exchange(
                *peer.signing_public_key(),
                ContactCard::new(name),
                SymmetricKey::generate(),
                0,
            ))
            .unwrap();
    }
    Device {
        dir,
        clock,
        storage,
        keys,
        me: Identity::create("Me", 0),
    }
}

/// Records whether the SMK still existed each time the network was used,
/// and can be told the network is down.
struct Network<'a> {
    keys: &'a MemoryKeyStorage,
    online: bool,
    smk_present_at_send: RefCell<Vec<bool>>,
}

impl<'a> Network<'a> {
    fn new(keys: &'a MemoryKeyStorage, online: bool) -> Self {
        Self {
            keys,
            online,
            smk_present_at_send: RefCell::new(Vec::new()),
        }
    }

    fn send(&self) -> Result<bool, ShredError> {
        self.smk_present_at_send
            .borrow_mut()
            .push(self.keys.load_key(SMK).unwrap().is_some());
        if self.online {
            Ok(true)
        } else {
            Err(ShredError::Deletion(DeletionError::DeletionFailed(
                "offline".into(),
            )))
        }
    }
}

impl PurgeSender for Network<'_> {
    fn send_purge(&mut self, _: &PreSignedPurgeRequest, _: u64) -> Result<bool, ShredError> {
        self.send()
    }
}

impl RevocationSender for Network<'_> {
    fn send_revocation_delivery(&mut self, _: &str, _: &str, _: u64) -> Result<bool, ShredError> {
        self.send()
    }
}

// @scenario: emergency_shred :: Soft shred schedules deletion with grace period
#[test]
fn a_soft_shred_schedules_deletion_a_week_out_and_keeps_my_data() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());

    let token = shred.soft_shred().unwrap();

    assert_eq!(token.created_at(), START);
    let DeletionState::Scheduled { execute_at, .. } =
        DeletionManager::new(&d.storage).deletion_state().unwrap()
    else {
        panic!("deletion is scheduled");
    };
    assert_eq!(execute_at, START + WEEK.as_secs());
    assert_eq!(d.storage.contacts().list_contacts().unwrap().len(), 2);
    assert!(d.keys.load_key(SMK).unwrap().is_some());
}

// @scenario: emergency_shred :: Soft shred cannot proceed before grace period expires
#[test]
fn a_hard_shred_a_day_early_is_refused_and_destroys_nothing() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());
    let token = shred.soft_shred().unwrap();
    d.clock.advance(WEEK - Duration::from_secs(24 * 60 * 60));

    let refused = shred.hard_shred(token, None, None);

    assert!(matches!(
        refused,
        Err(ShredError::Deletion(DeletionError::GracePeriodNotElapsed))
    ));
    assert!(d.keys.load_key(SMK).unwrap().is_some());
    assert_eq!(d.storage.contacts().list_contacts().unwrap().len(), 2);
}

// @scenario: emergency_shred :: Hard shred after grace period
// @scenario: emergency_shred :: Hard shred attempts network notifications before destruction
#[test]
fn after_the_week_a_hard_shred_notifies_while_keys_live_then_destroys_everything() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());
    let token = shred.soft_shred().unwrap();
    d.clock.advance(WEEK);
    let (mut relay, mut mailboxes) = (Network::new(&d.keys, true), Network::new(&d.keys, true));

    let report = shred
        .hard_shred(token, Some(&mut relay), Some(&mut mailboxes))
        .unwrap();

    assert_eq!(report.contacts_notified, 2);
    assert!(report.relay_purge_sent);
    let sends: Vec<bool> = [relay, mailboxes]
        .iter()
        .flat_map(|n| n.smk_present_at_send.borrow().clone())
        .collect();
    assert_eq!(
        sends,
        [true, true, true],
        "every send happened before key destruction"
    );
    assert!(report.smk_destroyed && report.sqlite_destroyed && report.data_dir_deleted);
    assert!(d.keys.load_key(SMK).unwrap().is_none());
    for leftover in [
        "vauchi.db",
        "vauchi.db-wal",
        "vauchi.db-shm",
        "identity.json",
    ] {
        assert!(!d.dir.path().join(leftover).exists(), "{leftover} remains");
    }
}

// @scenario: emergency_shred :: Hard shred attempts network notifications before destruction
#[test]
fn a_hard_shred_with_the_network_down_records_the_failures_and_still_destroys() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());
    let token = shred.soft_shred().unwrap();
    d.clock.advance(WEEK);
    let (mut relay, mut mailboxes) = (Network::new(&d.keys, false), Network::new(&d.keys, false));

    let report = shred
        .hard_shred(token, Some(&mut relay), Some(&mut mailboxes))
        .unwrap();

    assert_eq!(
        (report.contacts_notified, report.relay_purge_sent),
        (0, false)
    );
    assert!(report.smk_destroyed && report.sqlite_destroyed);
    assert!(shred.verify_shred().all_clear);
}

// @scenario: emergency_shred :: Panic shred when network is unavailable
// @scenario: emergency_shred :: Shred verification audits completeness
#[test]
fn a_panic_shred_offline_destroys_everything_and_reports_no_notifications() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());
    let (mut relay, mut mailboxes) = (Network::new(&d.keys, false), Network::new(&d.keys, false));

    let report = shred
        .panic_shred(Some(&mut relay), Some(&mut mailboxes))
        .unwrap();

    assert_eq!(
        (report.contacts_notified, report.relay_purge_sent),
        (0, false)
    );
    let audit = shred.verify_shred();
    assert_eq!(
        (
            audit.smk_absent,
            audit.database_absent,
            audit.data_dir_absent,
            audit.pre_signed_absent,
            audit.all_clear,
        ),
        (true, true, true, true, true)
    );
    assert!(!d.dir.path().join("identity.json").exists());
}

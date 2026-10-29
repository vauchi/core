// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hard shred's relay purge must be one the relay accepts when it arrives:
//! the relay refuses a purge whose timestamp is more than 60 s from its clock
//! (`relay/src/handler/verify.rs`, `TIMESTAMP_WINDOW`), and a hard shred runs
//! a week after the soft shred that stored the pre-signed purge
//! (vauchi/private#899). A panic shred sends nothing (#910).

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::api::{PreSignedPurgeRequest, PurgeSender, ShredError, ShredManager};
use vauchi_core::clock::FakeClock;
use vauchi_core::crypto::{PublicKey, Signature, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, Storage};

const START: u64 = 1_700_000_000;
const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RELAY_TIMESTAMP_WINDOW: u64 = 60;

/// Applies the relay's purge check to every purge it is handed.
#[derive(Default)]
struct Relay {
    verdicts: Vec<Result<(), String>>,
}

impl PurgeSender for Relay {
    fn send_purge(&mut self, purge: &PreSignedPurgeRequest, now: u64) -> Result<bool, ShredError> {
        let verdict = relay_accepts(purge, now);
        let accepted = verdict.is_ok();
        self.verdicts.push(verdict);
        Ok(accepted)
    }
}

fn relay_accepts(purge: &PreSignedPurgeRequest, now: u64) -> Result<(), String> {
    let age = now.abs_diff(purge.timestamp);
    if age > RELAY_TIMESTAMP_WINDOW {
        return Err(format!(
            "purge timestamp outside acceptable window ({age} s)"
        ));
    }
    let mut message = Vec::with_capacity(72);
    message.extend_from_slice(&purge.public_key);
    message.extend_from_slice(&purge.purge_token);
    message.extend_from_slice(&purge.timestamp.to_be_bytes());
    let signature: [u8; 64] = purge
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    if PublicKey::from_bytes(purge.public_key).verify(&message, &Signature::from_bytes(signature)) {
        Ok(())
    } else {
        Err("invalid purge signature".to_string())
    }
}

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
    let storage = Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate())
        .unwrap()
        .with_clock(clock.clone());
    Device {
        dir,
        clock,
        storage,
        keys: MemoryKeyStorage::new(),
        me: Identity::create("Me", 0),
    }
}

// @internal
#[test]
fn a_hard_shred_a_week_after_the_soft_shred_sends_a_purge_the_relay_accepts() {
    let d = device();
    let shred = ShredManager::new(&d.storage, &d.keys, &d.me, d.dir.path());
    let token = shred.soft_shred().unwrap();
    d.clock.advance(WEEK);
    let mut relay = Relay::default();

    let report = shred.hard_shred(token, Some(&mut relay), None).unwrap();

    assert_eq!(relay.verdicts, vec![Ok(())]);
    assert!(report.relay_purge_sent);
}

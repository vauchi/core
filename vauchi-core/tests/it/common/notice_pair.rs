// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One direction of the notice core as the tests drive it: a sender and a
//! receiver, the seed epoch's keys as a copy holds them, and records as a
//! holder of those keys can forge them (vauchi/private#1281).

use vauchi_core::crypto::X25519KeyPair;
use vauchi_core::notice::chain::PERIOD_SECONDS;
use vauchi_core::notice::direction::{Alarm, Receiver, Sender, week};
use vauchi_core::notice::keys::{
    Epoch, Label, opening_address, proof_tag, refresh_address, seed_epoch,
};
use vauchi_core::notice::record::Record;
use vauchi_core::notice::seal::{NOTICE_BYTES, seal};

pub(crate) const START: u64 = 1_800_000_000;
pub(crate) const DAY: u64 = 86_400;
pub(crate) const WEEK: u64 = PERIOD_SECONDS;

pub(crate) type Notice = [u8; NOTICE_BYTES];

pub(crate) struct Pair {
    pub(crate) sender: Sender,
    pub(crate) receiver: Receiver,
    /// The seed epoch's keys as a copy of either device holds them.
    pub(crate) copied_seed: Epoch,
}

pub(crate) fn pair() -> Pair {
    let sender_device = X25519KeyPair::generate();
    let receiver_device = X25519KeyPair::generate();
    let label = Label::new(sender_device.public_key(), receiver_device.public_key());
    let handshake = X25519KeyPair::generate()
        .diffie_hellman(X25519KeyPair::generate().public_key())
        .expect("contributory");
    let copied_seed = seed_epoch(&handshake, &label);
    let sender_public = *sender_device.public_key();
    let receiver_public = *receiver_device.public_key();
    Pair {
        sender: Sender::new(
            seed_epoch(&handshake, &label),
            label.clone(),
            sender_device,
            receiver_public,
            START,
        ),
        receiver: Receiver::new(
            seed_epoch(&handshake, &label),
            label,
            receiver_device,
            sender_public,
            START,
        ),
        copied_seed,
    }
}

/// A refresh as a holder of the seed epoch's keys can seal it.
pub(crate) fn forged_refresh(
    seed: &Epoch,
    now: u64,
    attempt: u8,
    number: u32,
    public: [u8; 32],
    names: [u8; 16],
) -> Notice {
    let at = refresh_address(seed.control_key(), week(now), attempt);
    let record = Record::Refresh {
        refresh_public: public,
        number,
        proof_tag: proof_tag(seed.proof_key(), number, &public, &names),
    };
    seal(&at, &record.to_plaintext())
}

/// The key ID in the honest sender's first opening on refresh 1, as a
/// holder of the seed epoch's keys reads it.
pub(crate) fn honest_key_id(p: &Pair, honest: &[Notice]) -> [u8; 8] {
    let opened = vauchi_core::notice::seal::open(
        &opening_address(p.copied_seed.opening_key(), 1, 0),
        &honest[0],
    )
    .expect("the honest opening");
    match Record::from_plaintext(&opened) {
        Ok(Record::Opening { key_id, .. }) => key_id,
        _ => panic!("not an opening"),
    }
}

/// An opening as a holder of the seed epoch's keys can seal it.
pub(crate) fn forged_opening(
    seed: &Epoch,
    attempt: u8,
    number: u32,
    key_id: [u8; 8],
    confirmation: [u8; 16],
) -> Notice {
    let at = opening_address(seed.opening_key(), number, attempt);
    let record = Record::Opening {
        ephemeral_public: *X25519KeyPair::generate().public_key(),
        number,
        key_id,
        confirmation,
    };
    seal(&at, &record.to_plaintext())
}

impl Pair {
    /// The receiver's session at `now`: it reads what the gateway hands
    /// it and writes; the write is acknowledged.
    pub(crate) fn receiver_session(&mut self, deliver: &[Notice], now: u64) -> Vec<Notice> {
        self.receiver.read(deliver, now);
        let written = self.receiver.write(now);
        self.receiver.acknowledge();
        written
    }

    pub(crate) fn sender_session(&mut self, deliver: &[Notice], now: u64) -> Vec<Notice> {
        self.sender.read(deliver, now);
        let written = self.sender.write(now);
        self.sender.acknowledge();
        written
    }

    /// A week in which the gateway delivers everything at once.
    pub(crate) fn faithful_week(&mut self, now: u64) {
        let refreshes = self.receiver_session(&[], now);
        let answers = self.sender_session(&refreshes, now + DAY);
        self.receiver_session(&answers, now + 2 * DAY);
    }

    pub(crate) fn assert_no_alarm(&self) {
        assert_eq!(self.sender.alarms(), &[] as &[Alarm], "sender");
        assert_eq!(self.receiver.alarms(), &[] as &[Alarm], "receiver");
    }

    /// The sender writes in the epoch the receiver's newest refresh names.
    pub(crate) fn assert_healed(&self) {
        assert_eq!(Some(self.sender.writes_in()), self.receiver.newest_name());
    }
}

pub(crate) fn update(byte: u8) -> ([u8; 32], [u8; 31]) {
    ([byte; 32], [byte; 31])
}

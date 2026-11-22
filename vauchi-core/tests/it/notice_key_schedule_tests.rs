// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core's key schedule against vectors computed independently,
//! in Python, from the same protocol text (vauchi/private#1281).

use proptest::prelude::*;
use vauchi_core::crypto::X25519KeyPair;
use vauchi_core::notice::keys::{
    Epoch, Label, chain_step, key_id, open_as_receiver, open_as_sender, opening_address, proof_tag,
    refresh_address, seed_epoch,
};

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../fixtures/test-vectors/notice-core-vectors-v7.json"
    ))
    .expect("notice vectors")
}

fn bytes<const N: usize>(value: &serde_json::Value) -> [u8; N] {
    hex::decode(value.as_str().expect("hex string"))
        .expect("hex")
        .try_into()
        .expect("length")
}

fn key_pair(value: &serde_json::Value) -> X25519KeyPair {
    X25519KeyPair::from_bytes(bytes(value))
}

fn assert_epoch_matches(epoch: &Epoch, recorded: &serde_json::Value) {
    assert_eq!(epoch.root(), &bytes::<32>(&recorded["root"]));
    assert_eq!(epoch.notice_chain(), &bytes::<32>(&recorded["notice"]));
    assert_eq!(epoch.record_key(), &bytes::<32>(&recorded["record"]));
    assert_eq!(epoch.control_key(), &bytes::<32>(&recorded["control"]));
    assert_eq!(epoch.opening_key(), &bytes::<32>(&recorded["opening"]));
    assert_eq!(epoch.proof_key(), &bytes::<32>(&recorded["proof"]));
    assert_eq!(epoch.id(), bytes::<16>(&recorded["epoch_id"]));
}

struct Recorded {
    label: Label,
    seed: Epoch,
    sender_device: X25519KeyPair,
    receiver_device: X25519KeyPair,
    refresh: X25519KeyPair,
    ephemeral: X25519KeyPair,
}

fn recorded(v: &serde_json::Value) -> Recorded {
    let inputs = &v["inputs"];
    let sender_device = key_pair(&inputs["s_device_private"]);
    let receiver_device = key_pair(&inputs["r_device_private"]);
    let label = Label::new(sender_device.public_key(), receiver_device.public_key());
    let handshake = key_pair(&inputs["s_handshake_private"])
        .diffie_hellman(key_pair(&inputs["r_handshake_private"]).public_key())
        .expect("contributory");
    Recorded {
        seed: seed_epoch(&handshake, &label),
        label,
        sender_device,
        receiver_device,
        refresh: key_pair(&inputs["r_refresh_private"]),
        ephemeral: key_pair(&inputs["s_ephemeral_private"]),
    }
}

// @internal
#[test]
fn the_label_is_the_senders_device_key_then_the_receivers() {
    let v = vectors();
    let r = recorded(&v);
    assert_eq!(r.label.as_bytes(), &bytes::<64>(&v["label"]));
}

// @internal
#[test]
fn the_seed_epoch_matches_the_recorded_vector() {
    let v = vectors();
    let r = recorded(&v);
    assert_eq!(r.seed.number(), 0);
    assert_epoch_matches(&r.seed, &v["seed_epoch"]);
}

// @internal
#[test]
fn an_opening_derives_the_same_epoch_on_both_devices() {
    let v = vectors();
    let r = recorded(&v);
    let by_sender = open_as_sender(
        &r.seed,
        &r.label,
        1,
        r.refresh.public_key(),
        &r.ephemeral,
        &r.sender_device,
        r.receiver_device.public_key(),
    )
    .expect("sender opens");
    let by_receiver = open_as_receiver(
        &r.seed,
        &r.label,
        1,
        &r.refresh,
        r.ephemeral.public_key(),
        r.sender_device.public_key(),
        &r.receiver_device,
    )
    .expect("receiver opens");
    let opened = &v["opened"];
    assert_eq!(by_sender.confirmation, bytes::<16>(&opened["conf"]));
    assert_eq!(by_receiver.confirmation, by_sender.confirmation);
    assert_eq!(by_sender.epoch.number(), 1);
    assert_epoch_matches(&by_sender.epoch, &opened["epoch"]);
    assert_epoch_matches(&by_receiver.epoch, &opened["epoch"]);
    assert_eq!(
        key_id(r.refresh.public_key()),
        bytes::<8>(&opened["key_id"])
    );
}

// @internal
#[test]
fn an_opening_on_a_low_order_key_is_refused_on_both_devices() {
    let v = vectors();
    let r = recorded(&v);
    let low_order = [0u8; 32];
    assert!(
        open_as_sender(
            &r.seed,
            &r.label,
            1,
            &low_order,
            &r.ephemeral,
            &r.sender_device,
            r.receiver_device.public_key(),
        )
        .is_err()
    );
    assert!(
        open_as_receiver(
            &r.seed,
            &r.label,
            1,
            &r.refresh,
            &low_order,
            r.sender_device.public_key(),
            &r.receiver_device,
        )
        .is_err()
    );
}

// @internal
#[test]
fn chain_steps_match_the_recorded_vector() {
    let v = vectors();
    let mut chain_key = bytes::<32>(&v["seed_epoch"]["notice"]);
    for recorded in v["chain"].as_array().expect("chain") {
        let step = chain_step(&chain_key);
        assert_eq!(step.at.tag, bytes::<16>(&recorded["tag"]));
        assert_eq!(*step.at.key, bytes::<32>(&recorded["mk"]));
        assert_eq!(*step.next, bytes::<32>(&recorded["next"]));
        chain_key = *step.next;
    }
}

// @internal
#[test]
fn addresses_and_the_proof_tag_match_the_recorded_vector() {
    let v = vectors();
    let r = recorded(&v);
    let week = u32::try_from(v["inputs"]["week"].as_u64().expect("week")).expect("u32");
    let attempt = u8::try_from(v["inputs"]["attempt"].as_u64().expect("attempt")).expect("u8");

    let refresh = refresh_address(r.seed.control_key(), week, attempt);
    assert_eq!(refresh.tag, bytes::<16>(&v["refresh_address"]["tag"]));
    assert_eq!(*refresh.key, bytes::<32>(&v["refresh_address"]["mk"]));

    let opening = opening_address(r.seed.opening_key(), 1, attempt);
    assert_eq!(opening.tag, bytes::<16>(&v["opening_address"]["tag"]));
    assert_eq!(*opening.key, bytes::<32>(&v["opening_address"]["mk"]));

    assert_eq!(
        proof_tag(r.seed.proof_key(), 1, r.refresh.public_key(), &r.seed.id()),
        bytes::<16>(&v["proof_tag"])
    );
}

// @internal
#[test]
fn an_address_differs_by_week_number_attempt_and_kind() {
    let v = vectors();
    let r = recorded(&v);
    let control = r.seed.control_key();
    let base = refresh_address(control, 2954, 0).tag;
    assert_ne!(refresh_address(control, 2955, 0).tag, base);
    assert_ne!(refresh_address(control, 2954, 1).tag, base);
    assert_ne!(opening_address(control, 2954, 0).tag, base);
    assert_ne!(
        opening_address(r.seed.opening_key(), 1, 0).tag,
        opening_address(r.seed.opening_key(), 2, 0).tag
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal
    #[test]
    fn both_devices_derive_one_epoch_and_any_other_input_gives_another(
        sender_device in any::<[u8; 32]>(),
        receiver_device in any::<[u8; 32]>(),
        refresh in any::<[u8; 32]>(),
        ephemeral in any::<[u8; 32]>(),
        seed in any::<[u8; 32]>(),
        number in 1u32..,
    ) {
        let sender_device = X25519KeyPair::from_bytes(sender_device);
        let receiver_device = X25519KeyPair::from_bytes(receiver_device);
        let refresh = X25519KeyPair::from_bytes(refresh);
        let ephemeral = X25519KeyPair::from_bytes(ephemeral);
        let label = Label::new(sender_device.public_key(), receiver_device.public_key());
        let base = seed_epoch(&seed, &label);

        let by_sender = open_as_sender(
            &base, &label, number, refresh.public_key(), &ephemeral, &sender_device,
            receiver_device.public_key(),
        ).expect("sender opens");
        let by_receiver = open_as_receiver(
            &base, &label, number, &refresh, ephemeral.public_key(), sender_device.public_key(),
            &receiver_device,
        ).expect("receiver opens");
        prop_assert_eq!(by_sender.epoch.id(), by_receiver.epoch.id());
        prop_assert_eq!(by_sender.epoch.proof_key(), by_receiver.epoch.proof_key());
        prop_assert_eq!(by_sender.confirmation, by_receiver.confirmation);

        let other_number = open_as_sender(
            &base, &label, number.wrapping_add(1).max(1), refresh.public_key(), &ephemeral,
            &sender_device, receiver_device.public_key(),
        ).expect("sender opens");
        prop_assert_ne!(other_number.epoch.id(), by_sender.epoch.id());
        prop_assert_ne!(other_number.confirmation, by_sender.confirmation);

        let reversed = Label::new(receiver_device.public_key(), sender_device.public_key());
        let other_direction = open_as_sender(
            &base, &reversed, number, refresh.public_key(), &ephemeral, &sender_device,
            receiver_device.public_key(),
        ).expect("sender opens");
        prop_assert_ne!(other_direction.epoch.id(), by_sender.epoch.id());
    }
}

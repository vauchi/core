// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The 96-byte notice: its synthetic-IV seal (ADR-019 amendment
//! 2026-11-22) and its three record kinds, against vectors computed
//! independently from the protocol text (vauchi/private#1281).

use proptest::prelude::*;
use vauchi_core::notice::keys::{Keyed, chain_step, opening_address, refresh_address};
use vauchi_core::notice::record::{Malformed, Record};
use vauchi_core::notice::seal::{NOTICE_BYTES, open, seal};
use zeroize::Zeroizing;

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

fn keyed(tag: [u8; 16], key: [u8; 32]) -> Keyed {
    Keyed {
        tag,
        key: Zeroizing::new(key),
    }
}

/// The three recorded notices with where each travels and what it says.
fn recorded() -> Vec<(&'static str, Keyed, Record, [u8; NOTICE_BYTES])> {
    let v = vectors();
    let inputs = &v["inputs"];
    let number = u32::try_from(inputs["refresh_number"].as_u64().expect("number")).expect("u32");
    let week = u32::try_from(inputs["week"].as_u64().expect("week")).expect("u32");
    let attempt = u8::try_from(inputs["attempt"].as_u64().expect("attempt")).expect("u8");
    let seed = &v["seed_epoch"];
    vec![
        (
            "update",
            chain_step(&bytes(&seed["notice"])).at,
            Record::UpdateKey {
                update_key: Zeroizing::new(bytes(&inputs["update_key"])),
                commitment: bytes(&inputs["commitment"]),
            },
            bytes(&v["notices"]["update"]),
        ),
        (
            "refresh",
            refresh_address(&bytes(&seed["control"]), week, attempt),
            Record::Refresh {
                refresh_public: bytes(&v["opened"]["r_refresh_public"]),
                number,
                proof_tag: bytes(&v["proof_tag"]),
            },
            bytes(&v["notices"]["refresh"]),
        ),
        (
            "opening",
            opening_address(&bytes(&seed["opening"]), number, attempt),
            Record::Opening {
                ephemeral_public: bytes(&v["opened"]["s_ephemeral_public"]),
                number,
                key_id: bytes(&v["opened"]["key_id"]),
                confirmation: bytes(&v["opened"]["conf"]),
            },
            bytes(&v["notices"]["opening"]),
        ),
    ]
}

// @internal
#[test]
fn each_record_kind_has_the_recorded_plaintext() {
    let v = vectors();
    for (kind, _, record, _) in recorded() {
        assert_eq!(
            *record.to_plaintext(),
            bytes::<64>(&v["plaintexts"][kind]),
            "{kind}"
        );
    }
}

// @internal
#[test]
fn each_record_seals_to_the_recorded_notice() {
    for (kind, at, record, notice) in recorded() {
        assert_eq!(seal(&at, &record.to_plaintext()), notice, "{kind}");
    }
}

// @internal
#[test]
fn each_recorded_notice_opens_to_its_record() {
    for (kind, at, record, notice) in recorded() {
        let plaintext = open(&at, &notice).expect(kind);
        assert_eq!(Record::from_plaintext(&plaintext), Ok(record), "{kind}");
    }
}

// @internal
#[test]
fn a_notice_with_any_one_bit_changed_does_not_open() {
    for (kind, at, _, notice) in recorded() {
        for bit in 0..NOTICE_BYTES * 8 {
            let mut altered = notice;
            altered[bit / 8] ^= 1 << (bit % 8);
            assert!(open(&at, &altered).is_none(), "{kind}, bit {bit}");
        }
    }
}

// @internal
#[test]
fn a_notice_does_not_open_under_another_key_or_at_another_tag() {
    let all = recorded();
    for (kind, at, _, notice) in &all {
        let mut other_key = *at.key;
        other_key[0] ^= 1;
        assert!(open(&keyed(at.tag, other_key), notice).is_none(), "{kind}");
        let mut other_tag = at.tag;
        other_tag[15] ^= 1;
        assert!(open(&keyed(other_tag, *at.key), notice).is_none(), "{kind}");
    }
    let (_, update_at, _, _) = &all[0];
    let (_, _, _, refresh_notice) = &all[1];
    assert!(open(update_at, refresh_notice).is_none());
}

// @internal
#[test]
fn a_plaintext_with_an_unknown_kind_or_nonzero_padding_is_malformed() {
    let mut unknown = [0u8; 64];
    unknown[0] = 0x03;
    assert_eq!(Record::from_plaintext(&unknown), Err(Malformed));
    unknown[0] = 0xff;
    assert_eq!(Record::from_plaintext(&unknown), Err(Malformed));

    for (kind, _, record, _) in recorded() {
        let padding_from = match record {
            Record::UpdateKey { .. } => continue,
            Record::Refresh { .. } => 53,
            Record::Opening { .. } => 61,
        };
        for index in padding_from..64 {
            let mut plaintext = *record.to_plaintext();
            plaintext[index] = 1;
            assert_eq!(
                Record::from_plaintext(&plaintext),
                Err(Malformed),
                "{kind}, byte {index}"
            );
        }
    }
}

proptest! {
    // @internal
    #[test]
    fn a_sealed_plaintext_opens_to_itself_and_a_key_used_twice_shows_only_equality(
        key in any::<[u8; 32]>(),
        tag in any::<[u8; 16]>(),
        first in any::<[u8; 64]>(),
        second in any::<[u8; 64]>(),
    ) {
        let at = keyed(tag, key);
        let sealed = seal(&at, &first);
        prop_assert_eq!(&sealed[..16], &tag[..]);
        prop_assert_eq!(*open(&at, &sealed).expect("opens"), first);
        prop_assert_eq!(seal(&at, &first), sealed);

        prop_assume!(first != second);
        let other = seal(&at, &second);
        // The synthetic IV, bytes 16..32, differs with the plaintext, so the
        // two keystreams differ and the XOR of the ciphertexts is not the
        // XOR of the plaintexts.
        prop_assert_ne!(&other[16..32], &sealed[16..32]);
        let xor_matches = (32..NOTICE_BYTES)
            .all(|i| sealed[i] ^ other[i] == first[i - 32] ^ second[i - 32]);
        prop_assert!(!xor_matches);
    }

    // @internal
    #[test]
    fn a_record_survives_its_plaintext(
        key in any::<[u8; 32]>(),
        commitment in any::<[u8; 31]>(),
        number in any::<u32>(),
        proof_tag in any::<[u8; 16]>(),
        key_id in any::<[u8; 8]>(),
    ) {
        let records = [
            Record::UpdateKey { update_key: Zeroizing::new(key), commitment },
            Record::Refresh { refresh_public: key, number, proof_tag },
            Record::Opening { ephemeral_public: key, number, key_id, confirmation: proof_tag },
        ];
        for record in records {
            let plaintext = record.to_plaintext();
            prop_assert_eq!(Record::from_plaintext(&plaintext), Ok(record));
        }
    }
}

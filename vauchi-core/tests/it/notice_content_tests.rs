// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content records: a card body in 576-byte records under an update key,
//! found by IDs that never repeat and bound by the notice's commitment
//! (protocol section 4; vauchi/private#1281).

use proptest::prelude::*;
use vauchi_core::notice::content::{
    Assembly, ContentError, MAX_BODY_BYTES, RECORD_BYTES, commitment, open_record, record_count,
    record_id, seal_record,
};

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../fixtures/test-vectors/notice-core-vectors-v7.json"
    ))
    .expect("notice vectors")
}

fn hex_bytes(value: &serde_json::Value) -> Vec<u8> {
    hex::decode(value.as_str().expect("hex string")).expect("hex")
}

fn array<const N: usize>(value: &serde_json::Value) -> [u8; N] {
    hex_bytes(value).try_into().expect("length")
}

const KEY: [u8; 32] = [3; 32];
const NONCE: [u8; 24] = [5; 24];
const WEEK: u32 = 2954;

fn body(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 251) as u8).collect()
}

/// A distinct nonce per record: a nonce is never used twice under one key.
fn nonce_for(index: u16) -> [u8; 24] {
    let mut nonce = NONCE;
    nonce[..2].copy_from_slice(&index.to_be_bytes());
    nonce
}

/// Every record of a body, each sealed as transmission 0.
fn publication(key: &[u8; 32], body: &[u8], week: u32) -> Vec<[u8; RECORD_BYTES]> {
    (0..record_count(body.len()).expect("a body that fits"))
        .map(|index| seal_record(key, body, week, index, 0, &nonce_for(index)).expect("sealed"))
        .collect()
}

// @internal
#[test]
fn records_and_the_commitment_match_the_recorded_vector() {
    let v = vectors();
    let key: [u8; 32] = array(&v["inputs"]["update_key"]);
    let content = &v["content"];
    let body = hex_bytes(&content["body"]);
    let week = u32::try_from(content["week"].as_u64().expect("week")).expect("u32");
    assert_eq!(commitment(&key, &body), array::<31>(&content["commitment"]));
    for recorded in content["records"].as_array().expect("records") {
        let index = u16::try_from(recorded["index"].as_u64().expect("index")).expect("u16");
        let transmission = u8::try_from(recorded["transmission"].as_u64().expect("t")).expect("u8");
        let sealed = seal_record(
            &key,
            &body,
            week,
            index,
            transmission,
            &array(&recorded["nonce"]),
        )
        .expect("sealed");
        assert_eq!(
            sealed.to_vec(),
            hex_bytes(&recorded["record"]),
            "record {index}"
        );
    }
}

// @internal
#[test]
fn a_record_opens_under_its_id_and_tells_its_place() {
    let body = body(1_200);
    let sealed = seal_record(&KEY, &body, WEEK, 2, 1, &NONCE).expect("sealed");
    assert_eq!(&sealed[..16], &record_id(&KEY, WEEK, 2, 1));
    let chunk = open_record(&KEY, WEEK, 2, 1, &sealed).expect("opens");
    assert_eq!((chunk.index, chunk.count, chunk.length), (2, 3, 1_200));
    // Record 0 holds two 16-byte hashes before the body, so record 2 holds
    // the last 208 bytes.
    assert_eq!(&chunk.bytes[..208], &body[992..]);
    assert!(chunk.bytes[208..].iter().all(|byte| *byte == 0));
}

// @internal
#[test]
fn no_id_repeats_across_weeks_indices_and_transmissions() {
    let mut ids = std::collections::BTreeSet::new();
    for week in WEEK..WEEK + 4 {
        for index in 0..8 {
            for transmission in 0..4 {
                assert!(ids.insert(record_id(&KEY, week, index, transmission)));
            }
        }
    }
    assert!(ids.insert(record_id(&[4; 32], WEEK, 0, 0)));
}

// @internal
#[test]
fn a_record_altered_or_looked_up_under_another_id_or_key_does_not_open() {
    let body = body(700);
    let sealed = seal_record(&KEY, &body, WEEK, 1, 0, &NONCE).expect("sealed");
    for byte in [0usize, 15, 16, 39, 40, 300, 559, 560, 575] {
        let mut altered = sealed;
        altered[byte] ^= 1;
        assert!(
            open_record(&KEY, WEEK, 1, 0, &altered).is_none(),
            "byte {byte}"
        );
    }
    assert!(open_record(&KEY, WEEK, 0, 0, &sealed).is_none());
    assert!(open_record(&[4; 32], WEEK, 1, 0, &sealed).is_none());
}

// @internal
#[test]
fn a_body_must_be_one_to_3984_bytes_and_an_index_inside_it() {
    assert_eq!(record_count(0), Err(ContentError::BodySize));
    assert_eq!(record_count(1), Ok(1));
    assert_eq!(record_count(512), Ok(1));
    assert_eq!(record_count(513), Ok(2));
    assert_eq!(
        record_count(1_008),
        Ok(2),
        "two records hold 1,024 bytes less one hash"
    );
    assert_eq!(record_count(1_009), Ok(3));
    assert_eq!(MAX_BODY_BYTES, 3_984);
    assert_eq!(record_count(MAX_BODY_BYTES), Ok(8));
    assert_eq!(
        record_count(MAX_BODY_BYTES + 1),
        Err(ContentError::BodySize)
    );
    assert_eq!(
        seal_record(&KEY, &[], WEEK, 0, 0, &NONCE).err(),
        Some(ContentError::BodySize)
    );
    assert_eq!(
        seal_record(&KEY, &body(MAX_BODY_BYTES + 1), WEEK, 0, 0, &NONCE).err(),
        Some(ContentError::BodySize)
    );
    assert_eq!(
        seal_record(&KEY, &body(600), WEEK, 2, 0, &NONCE).err(),
        Some(ContentError::Index)
    );
    assert_eq!(
        seal_record(&KEY, &body(600), WEEK, 0, 4, &NONCE).err(),
        Some(ContentError::Transmission)
    );
}

// @internal
#[test]
fn chunks_from_different_publications_complete_one_body() {
    let body = body(1_500);
    let expected = commitment(&KEY, &body);
    let first_week = publication(&KEY, &body, WEEK);
    let second = seal_record(&KEY, &body, WEEK + 1, 1, 2, &[9; 24]).expect("sealed");

    let mut assembly = Assembly::new(expected);
    let open = |record: &[u8; RECORD_BYTES], week, index, transmission| {
        open_record(&KEY, week, index, transmission, record).expect("opens")
    };
    assert_eq!(assembly.add(&KEY, open(&first_week[0], WEEK, 0, 0)), Ok(()));
    assert_eq!(assembly.missing(), vec![1, 2]);
    assert!(assembly.complete().is_none());
    assert_eq!(assembly.add(&KEY, open(&first_week[2], WEEK, 2, 0)), Ok(()));
    assert_eq!(assembly.add(&KEY, open(&second, WEEK + 1, 1, 2)), Ok(()));
    assert_eq!(assembly.missing(), Vec::<u16>::new());
    assert_eq!(assembly.complete().expect("complete").to_vec(), body);
}

// @internal
#[test]
fn a_chunk_that_disagrees_or_a_body_off_its_commitment_is_refused() {
    let body = body(1_500);
    let other = self::body(700);
    let mut assembly = Assembly::new(commitment(&KEY, &body));
    let chunk = |body: &[u8], index| {
        let sealed = seal_record(&KEY, body, WEEK, index, 0, &nonce_for(index)).expect("sealed");
        open_record(&KEY, WEEK, index, 0, &sealed).expect("opens")
    };
    assert_eq!(assembly.add(&KEY, chunk(&body, 0)), Ok(()));
    assert_eq!(
        assembly.add(&KEY, chunk(&other, 1)),
        Err(ContentError::Disagrees)
    );
    assert_eq!(assembly.missing(), vec![1, 2]);

    let mut off_commitment = Assembly::new([0; 31]);
    assert_eq!(
        off_commitment.add(&KEY, chunk(&body, 0)),
        Err(ContentError::Disagrees)
    );
    for index in 1..3 {
        assert_eq!(
            off_commitment.add(&KEY, chunk(&body, index)),
            Err(ContentError::NeedsFirstRecord)
        );
    }
    assert!(off_commitment.complete().is_none());
}

// Codex, reading the protocol text alone, 2026-11-22: a record must carry
// the index its ID was wanted for. Only a holder of the update key can
// seal one that does not, so this seals one by hand.
// @internal
#[test]
fn a_record_carrying_another_index_than_its_id_is_refused() {
    use chacha20poly1305::XChaCha20Poly1305;
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use vauchi_core::crypto::HKDF;

    let body = body(1_500);
    let honest = seal_record(&KEY, &body, WEEK, 1, 0, &nonce_for(1)).expect("sealed");
    let record_key = HKDF::derive_array::<32>(None, &KEY, b"Vauchi_Update_Record_v5");
    let id_of_index_0 = record_id(&KEY, WEEK, 0, 0);
    let mut plaintext = Vec::new();
    plaintext.extend_from_slice(&1u16.to_be_bytes());
    plaintext.extend_from_slice(&3u16.to_be_bytes());
    plaintext.extend_from_slice(&1_500u32.to_be_bytes());
    plaintext.extend_from_slice(&body[512..1_024]);
    let sealed = XChaCha20Poly1305::new(record_key.as_ref().into())
        .encrypt(
            chacha20poly1305::XNonce::from_slice(&NONCE),
            Payload {
                msg: &plaintext,
                aad: &id_of_index_0,
            },
        )
        .expect("seals");
    let mut forged = [0u8; RECORD_BYTES];
    for (to, from) in forged.iter_mut().zip(
        id_of_index_0
            .iter()
            .chain(NONCE.iter())
            .chain(sealed.iter()),
    ) {
        *to = *from;
    }

    assert!(
        open_record(&KEY, WEEK, 1, 0, &honest).is_some(),
        "the honest record opens"
    );
    assert!(
        open_record(&KEY, WEEK, 0, 0, &forged).is_none(),
        "index 1 under the ID of index 0"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal
    #[test]
    fn any_body_survives_any_arrival_order(
        length in 1usize..=MAX_BODY_BYTES,
        seed in any::<u64>(),
        key in any::<[u8; 32]>(),
    ) {
        let body: Vec<u8> = (0..length).map(|i| (seed.wrapping_mul(i as u64 + 1) >> 7) as u8).collect();
        let mut records: Vec<(u16, [u8; RECORD_BYTES])> = publication(&key, &body, WEEK)
            .into_iter()
            .enumerate()
            .map(|(index, record)| (index as u16, record))
            .collect();
        let rotate = (seed as usize) % records.len();
        records.rotate_left(rotate);
        if seed % 2 == 0 { records.reverse(); }

        // A later chunk before record 0 is refused and changes nothing;
        // it comes again, as in a later publication.
        let mut assembly = Assembly::new(commitment(&key, &body));
        let mut again = Vec::new();
        for (index, record) in &records {
            prop_assert!(assembly.complete().is_none());
            let chunk = open_record(&key, WEEK, *index, 0, record).expect("opens");
            match assembly.add(&key, chunk) {
                Ok(()) => {}
                Err(ContentError::NeedsFirstRecord) => again.push((*index, *record)),
                Err(other) => prop_assert!(false, "refused: {:?}", other),
            }
        }
        for (index, record) in &again {
            let chunk = open_record(&key, WEEK, *index, 0, record).expect("opens");
            prop_assert_eq!(assembly.add(&key, chunk), Ok(()));
        }
        prop_assert_eq!(assembly.complete().expect("complete").to_vec(), body);
    }
}

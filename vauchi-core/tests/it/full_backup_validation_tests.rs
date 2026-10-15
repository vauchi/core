// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Authenticated but structurally invalid snapshots must fail before writes.

use std::io::{Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde_json::{Value, json};
use vauchi_core::contact::{Contact, Group};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{
    HKDF, SymmetricKey, decrypt_with_ad, derive_key_argon2id, encrypt_with_ad,
};
use vauchi_core::{ContactField, FieldType, Vauchi};
use zeroize::Zeroizing;

const PASSWORD: &str = "correct-horse-battery-staple";

fn key(salt: &[u8; 16]) -> SymmetricKey {
    let base = derive_key_argon2id(PASSWORD.as_bytes(), salt).unwrap();
    let prk = HKDF::extract(None, base.as_bytes());
    let expanded = Zeroizing::new(HKDF::expand(&prk, b"vauchi-backup-v6-state", 32).unwrap());
    SymmetricKey::try_from_bytes(expanded.as_slice().try_into().unwrap()).unwrap()
}

fn fixture() -> Value {
    let mut source = Vauchi::in_memory().unwrap();
    source.create_identity("Alice").unwrap();
    let field = ContactField::new(FieldType::Email, "Email", "alice@example.test", 10);
    let field_id = field.id().to_owned();
    source.add_own_field(field).unwrap();
    let bob = Contact::from_exchange(
        [0x42; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        10,
    );
    source.update_contact(&bob).unwrap();
    let mut group = Group::new("work".into(), "Work", 10);
    group.add_contact(bob.id(), 20);
    group.add_visible_field(&field_id, 20);
    source.storage().labels().save_group(&group).unwrap();
    source
        .storage()
        .labels()
        .save_contact_override(bob.id(), &field_id, false)
        .unwrap();
    let data = hex::decode(source.export_full_backup(PASSWORD).unwrap()).unwrap();
    assert_eq!(data[0], 6);
    let decrypted = Zeroizing::new(
        decrypt_with_ad(
            &key(data[1..17].try_into().unwrap()),
            &data[17..],
            &data[..17],
        )
        .unwrap(),
    );
    let mut plaintext = Zeroizing::new(Vec::new());
    ZlibDecoder::new(&decrypted[..])
        .read_to_end(&mut plaintext)
        .unwrap();
    serde_json::from_slice(&plaintext).unwrap()
}

fn encode(value: &Value) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    let plaintext = Zeroizing::new(serde_json::to_vec(value).unwrap());
    encoder.write_all(&plaintext).unwrap();
    let compressed = Zeroizing::new(encoder.finish().unwrap());
    let salt = [0x24; 16];
    let mut header = vec![6];
    header.extend(salt);
    let ciphertext = encrypt_with_ad(&key(&salt), &compressed, &header).unwrap();
    header.extend(ciphertext);
    hex::encode(header)
}

// @scenario: backup_format_versioning :: Full backup import is atomic
#[test]
fn full_backup_validation_rejects_invalid_state_before_mutation() {
    let valid = fixture();
    let mut target = Vauchi::in_memory().unwrap();
    target
        .import_full_backup(&encode(&valid), PASSWORD)
        .unwrap();
    assert_eq!(target.identity().unwrap().display_name(), "Alice");
    let mut accepted = Vec::new();
    for case in 0..14 {
        let mut malformed = valid.clone();
        match case {
            0 => malformed["version"] = json!(7),
            1 => {
                let item = malformed["contacts"][0].clone();
                malformed["contacts"].as_array_mut().unwrap().push(item);
            }
            2 => malformed["contacts"][0]["entry"]["id"] = json!("mismatched-id"),
            3 => {
                let item = malformed["groups"][0].clone();
                malformed["groups"].as_array_mut().unwrap().push(item);
            }
            4 => malformed["groups"][0]["contact_ids"] = json!(["missing-contact"]),
            5 => malformed["groups"][0]["visible_fields"] = json!(["missing-field"]),
            6 => malformed["overrides"][0]["contact_id"] = json!("missing-contact"),
            7 => malformed["overrides"][0]["field_id"] = json!("missing-field"),
            8 => {
                let item = malformed["overrides"][0].clone();
                malformed["overrides"].as_array_mut().unwrap().push(item);
            }
            9 => malformed["groups"][0]["name"] = json!(" "),
            10 => malformed["groups"][0]["display_name_override"] = json!("x".repeat(101)),
            11 => malformed["groups"][0]["bio_override"] = json!("x".repeat(161)),
            12 => malformed["own_card"]["fields"][0]["value"] = json!("x".repeat(1001)),
            13 => malformed["contacts"][0]["relationship"] = Value::Null,
            _ => unreachable!(),
        }
        let mut target = Vauchi::in_memory().unwrap();
        if target
            .import_full_backup(&encode(&malformed), PASSWORD)
            .is_ok()
        {
            accepted.push(case);
        } else {
            assert!(target.identity().is_none(), "case {case}");
            assert!(
                target
                    .storage()
                    .identity()
                    .load_identity()
                    .unwrap()
                    .is_none(),
                "case {case}"
            );
            assert!(
                target
                    .storage()
                    .contacts()
                    .load_own_card()
                    .unwrap()
                    .is_none(),
                "case {case}"
            );
            assert_eq!(
                target.storage().contacts().count_contacts().unwrap(),
                0,
                "case {case}"
            );
        }
    }
    assert!(accepted.is_empty(), "invalid cases accepted: {accepted:?}");
}

// @scenario: backup_format_versioning :: Corrupted backup is detected
#[test]
fn full_backup_validation_authenticates_header_and_ciphertext() {
    let bytes = hex::decode(encode(&fixture())).unwrap();
    for index in [0, 1, 16, 17, bytes.len() - 1] {
        let mut corrupt = bytes.clone();
        corrupt[index] ^= 1;
        let mut target = Vauchi::in_memory().unwrap();
        assert!(
            target
                .import_full_backup(&hex::encode(corrupt), PASSWORD)
                .is_err()
        );
        assert!(
            target
                .storage()
                .identity()
                .load_identity()
                .unwrap()
                .is_none()
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The v7 full-backup vector in `vauchi-protocol-vectors-v1.json` (spec
//! keys-and-cards D4): a clear header of version, file id and two salts,
//! then two payloads of one size, each sealed with the header as associated
//! data. The vector records the real payload's slot, salt and keys; the
//! other payload is sealed under a random key and opens under nothing.

use serde::{Deserialize, Serialize};
use vauchi_core::crypto::{HKDF, SymmetricKey, decrypt_with_ad};

#[cfg(not(feature = "test-kdf"))]
use vauchi_core::Vauchi;

use crate::protocol_test_vectors_tests::{Argon2Params, hex32, load};
#[cfg(not(feature = "test-kdf"))]
use crate::protocol_test_vectors_tests::{V6_SEED_BYTE, seed, vauchi_with_vector_identity};

pub(crate) const BACKUP_V7_INFO: &str = "vauchi-backup-v7-payload";
const HEADER_LENGTH: usize = 49;
const LENGTH_PREFIX: usize = 4;
#[cfg(not(feature = "test-kdf"))]
const V7_PASSWORD: &str = "correct horse battery staple";

#[derive(Serialize, Deserialize)]
pub(crate) struct BackupV7Vector {
    password: String,
    argon2id: Argon2Params,
    hkdf_info: String,
    display_name: String,
    master_seed: String,
    device_index: u32,
    device_name: String,
    created_at: u64,
    file_id: String,
    salts: [String; 2],
    real_slot: usize,
    argon2id_base_key: String,
    key: String,
    header: String,
    backup: String,
}

impl BackupV7Vector {
    fn payloads<'a>(&self, backup: &'a [u8]) -> [&'a [u8]; 2] {
        let (first, second) = backup[HEADER_LENGTH..].split_at((backup.len() - HEADER_LENGTH) / 2);
        [first, second]
    }
}

/// Derives the v7 payload key from an Argon2id base key, as D4 states.
fn payload_key(base_key: &[u8; 32], info: &str) -> SymmetricKey {
    let prk = HKDF::extract(None, base_key);
    let key = HKDF::expand(&prk, info.as_bytes(), 32).expect("expand");
    SymmetricKey::from_bytes(key.try_into().expect("32 bytes"))
}

#[cfg(not(feature = "test-kdf"))]
pub(crate) fn compute_backup_v7() -> BackupV7Vector {
    let vauchi = vauchi_with_vector_identity();
    let backup =
        hex::decode(vauchi.export_full_backup(V7_PASSWORD).expect("v7 export")).expect("hex");
    let header = &backup[..HEADER_LENGTH];
    let salts: [[u8; 16]; 2] = [
        header[17..33].try_into().expect("salt"),
        header[33..49].try_into().expect("salt"),
    ];
    let (first, second) = backup[HEADER_LENGTH..].split_at((backup.len() - HEADER_LENGTH) / 2);
    let opens = |salt: &[u8; 16], payload: &[u8]| {
        let base =
            vauchi_core::crypto::derive_key_argon2id(V7_PASSWORD.as_bytes(), salt).expect("kdf");
        let base: [u8; 32] = *base.as_bytes();
        decrypt_with_ad(&payload_key(&base, BACKUP_V7_INFO), payload, header)
            .is_ok()
            .then_some(base)
    };
    let (real_slot, base_key) = opens(&salts[0], first)
        .map(|base| (0, base))
        .or_else(|| opens(&salts[1], second).map(|base| (1, base)))
        .expect("one payload opens under the password");
    let prk = HKDF::extract(None, &base_key);
    let key = HKDF::expand(&prk, BACKUP_V7_INFO.as_bytes(), 32).expect("expand");
    let identity = vauchi.identity().expect("identity");
    BackupV7Vector {
        password: V7_PASSWORD.to_string(),
        argon2id: Argon2Params {
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 4,
            output_bytes: 32,
        },
        hkdf_info: BACKUP_V7_INFO.to_string(),
        display_name: identity.display_name().to_string(),
        master_seed: hex::encode(seed(V6_SEED_BYTE)),
        device_index: identity.device_index(),
        device_name: "vector-device".to_string(),
        created_at: crate::protocol_test_vectors_tests::V6_CREATED_AT,
        file_id: hex::encode(&header[1..17]),
        salts: [hex::encode(salts[0]), hex::encode(salts[1])],
        real_slot,
        argon2id_base_key: hex::encode(base_key),
        key: hex::encode(&key),
        header: hex::encode(header),
        backup: hex::encode(&backup),
    }
}

// @claim: S13
/// Opens the recorded v7 backup's real payload from the recorded Argon2id
/// output with the header as associated data, and shows what binds it: a
/// changed file id fails the tag, the other payload is the same size and
/// opens under nothing, and the framed snapshot is version 7.
// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn backup_v7_vector_opens_its_real_payload_under_its_header_only() {
    let v = load().backup_v7;
    let backup = hex::decode(&v.backup).expect("backup");
    let header = hex::decode(&v.header).expect("header");
    assert_eq!(backup[0], 0x07, "v7 version byte");
    assert_eq!(&backup[..HEADER_LENGTH], header.as_slice());
    assert_eq!(hex::encode(&header[1..17]), v.file_id);
    assert_eq!(hex::encode(&header[17..33]), v.salts[0]);
    assert_eq!(hex::encode(&header[33..49]), v.salts[1]);
    assert_eq!(v.hkdf_info, BACKUP_V7_INFO);

    let key = payload_key(&hex32(&v.argon2id_base_key), &v.hkdf_info);
    assert_eq!(hex::encode(key.as_bytes()), v.key);
    let payloads = v.payloads(&backup);
    assert_eq!(payloads[0].len(), payloads[1].len(), "one bucket size");
    assert!(
        decrypt_with_ad(&key, payloads[1 - v.real_slot], &header).is_err(),
        "the other payload does not open under the real key"
    );
    let mut other_header = header.clone();
    other_header[1] ^= 0x01;
    assert!(
        decrypt_with_ad(&key, payloads[v.real_slot], &other_header).is_err(),
        "a changed file id in the associated data must fail the tag"
    );

    let framed = decrypt_with_ad(&key, payloads[v.real_slot], &header).expect("decrypt");
    let length = u32::from_be_bytes(framed[..LENGTH_PREFIX].try_into().expect("prefix")) as usize;
    assert!(
        framed[LENGTH_PREFIX + length..].iter().all(|&b| b == 0),
        "zero padding"
    );
    let compressed = &framed[LENGTH_PREFIX..LENGTH_PREFIX + length];
    assert_eq!(compressed[0], 0x78, "zlib stream");
    let mut json = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(compressed), &mut json)
        .expect("inflate");
    let snapshot: serde_json::Value = serde_json::from_slice(&json).expect("json");
    assert_eq!(snapshot["version"], 7);
    assert_eq!(snapshot["created_at"], v.created_at);
    assert_eq!(snapshot["identity"]["display_name"], v.display_name);
    assert_eq!(snapshot["identity"]["device_name"], v.device_name);
    assert_eq!(snapshot["identity"]["device_index"], v.device_index);
}

// @claim: S13
/// The whole v7 import, password in, on a fresh installation; a wrong
/// password restores nothing.
// @scenario: security :: Correct algorithms used
// @internal
#[cfg(not(feature = "test-kdf"))]
#[test]
fn backup_v7_vector_imports_with_production_argon2id_and_refuses_a_wrong_password() {
    let v = load().backup_v7;
    let salt = hex::decode(&v.salts[v.real_slot]).expect("salt");
    let base_key = vauchi_core::crypto::derive_key_argon2id(v.password.as_bytes(), salt.as_slice())
        .expect("argon2id");
    assert_eq!(hex::encode(base_key.as_bytes()), v.argon2id_base_key);

    let mut wrong = Vauchi::in_memory().expect("in-memory vauchi");
    assert!(
        wrong
            .import_full_backup(&v.backup, "incorrect horse battery staple")
            .is_err(),
        "a wrong password must not restore the identity"
    );
    assert!(wrong.identity().is_none());

    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi
        .import_full_backup(&v.backup, &v.password)
        .expect("v7 import");
    let identity = vauchi.identity().expect("restored identity");
    assert_eq!(hex::encode(identity.master_seed()), v.master_seed);
    assert_eq!(identity.display_name(), v.display_name);
    assert_eq!(identity.device_info().device_name(), v.device_name);
    assert_restore_registered_a_fresh_device(&vauchi, v.device_index);
}

#[cfg(not(feature = "test-kdf"))]
pub(crate) fn assert_restore_registered_a_fresh_device(vauchi: &Vauchi, backed_up_index: u32) {
    let identity = vauchi.identity().expect("restored identity");
    let backed_up_device = vauchi_core::identity::device::DeviceInfo::derive(
        identity.master_seed(),
        backed_up_index,
        String::new(),
        0,
    );
    assert_ne!(identity.device_id(), backed_up_device.device_id());
    assert_eq!(identity.device_index() & 0x8000_0000, 0x8000_0000);
    let registry = vauchi
        .storage()
        .device()
        .load_device_registry()
        .expect("registry storage")
        .expect("restore persisted its registry");
    assert_eq!(registry.active_count(), 1);
    assert!(registry.find_device(identity.device_id()).is_some());
    assert!(registry.find_device(backed_up_device.device_id()).is_none());
    assert!(registry.verify(&identity.signing_keypair().public_key()));
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
// @internal
#[cfg(not(feature = "test-kdf"))]
#[test]
fn newly_generated_backup_v7_vector_imports_as_a_fresh_device() {
    let v = compute_backup_v7();
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi
        .import_full_backup(&v.backup, &v.password)
        .expect("fresh v7 import");

    let identity = vauchi.identity().expect("restored identity");
    assert_eq!(hex::encode(identity.master_seed()), v.master_seed);
    assert_eq!(identity.display_name(), v.display_name);
    assert_restore_registered_a_fresh_device(&vauchi, v.device_index);
}

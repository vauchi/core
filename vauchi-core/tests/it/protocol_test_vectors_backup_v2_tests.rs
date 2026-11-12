// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(not(feature = "test-kdf"))]
use vauchi_core::crypto::{SymmetricKey, decrypt, encrypt};
#[cfg(not(feature = "test-kdf"))]
use vauchi_core::identity::{Identity, IdentityBackup, IdentityError};

// The recorded Argon2id output fixes all four parameters independently of
// the production KDF; cucumber enables test-kdf and cannot check this contract.
// @scenario: backup_format_versioning :: V2 backup uses OWASP-recommended Argon2id parameters
#[cfg(not(feature = "test-kdf"))]
#[test]
fn v2_backup_import_uses_the_recorded_production_argon2id_key() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/test-vectors/vauchi-protocol-vectors-v1.json"
    ))
    .expect("protocol vectors");
    let vector = &vectors["backup"];
    assert_eq!(
        vector["argon2id"],
        serde_json::json!({
            "memory_kib": 65536, "iterations": 3, "parallelism": 4, "output_bytes": 32
        })
    );
    let key_bytes: [u8; 32] = hex::decode(vector["argon2id_base_key"].as_str().unwrap())
        .unwrap()
        .try_into()
        .expect("32-byte known-answer key");
    let key = SymmetricKey::from_bytes(key_bytes);
    let seed = [0x42; 32];
    let mut plaintext = Vec::from(6_u32.to_le_bytes());
    plaintext.extend_from_slice(b"Vector");
    plaintext.extend_from_slice(&seed);
    plaintext.extend_from_slice(&0_u32.to_le_bytes());
    plaintext.extend_from_slice(&6_u32.to_le_bytes());
    plaintext.extend_from_slice(b"device");
    let mut bytes = vec![0x02];
    bytes.extend_from_slice(&hex::decode(vector["salt"].as_str().unwrap()).unwrap());
    bytes.extend_from_slice(&encrypt(&key, &plaintext).expect("seal v2 fixture"));
    let backup = IdentityBackup::new(bytes);

    let identity = Identity::import_backup(&backup, vector["password"].as_str().unwrap(), 0)
        .expect("v2 import must reproduce the production Argon2id key");
    assert_eq!(identity.master_seed(), &seed);
    assert_eq!(identity.display_name(), "Vector");
    assert_eq!(identity.device_info().device_name(), "device");
    let exported = identity
        .export_backup(vector["password"].as_str().unwrap())
        .expect("export v2 backup");
    let exported = exported.as_bytes();
    assert_eq!(exported[0], 0x02);
    let params = argon2::Params::new(65536, 3, 4, Some(32)).unwrap();
    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut export_key = [0; 32];
    argon2
        .hash_password_into(
            vector["password"].as_str().unwrap().as_bytes(),
            &exported[1..17],
            &mut export_key,
        )
        .expect("derive independent production key");
    assert_eq!(
        decrypt(&SymmetricKey::from_bytes(export_key), &exported[17..]).unwrap(),
        plaintext
    );
    assert!(matches!(
        Identity::import_backup(&backup, "wrong password", 0),
        Err(IdentityError::RestoreFailed)
    ));
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The optional fields that let one signed purge say whether the guardian
//! set goes too, and let a recovery proof name the identity that uploaded
//! it (#711 D2–D4). Old peers neither send nor expect them.

use proptest::prelude::*;
use vauchi_protocol::v2::{V2PurgeRequest, V2RecoveryStoreRequest};

fn purge_json(extra: &str) -> String {
    format!(
        r#"{{"recipient_id":"{}","public_key":"{}","purge_token":"{}","signature":"{}","timestamp":1700000000{extra}}}"#,
        "ab".repeat(32),
        "aa".repeat(32),
        "bb".repeat(32),
        "cc".repeat(64),
    )
}

fn recovery_store_json(extra: &str) -> String {
    format!(
        r#"{{"key_hash":"{}","proof_data":"cHJvb2Y="{extra}}}"#,
        "dd".repeat(32)
    )
}

fn owned_proof(owner_pk: &str, signature: &str) -> String {
    recovery_store_json(&format!(
        r#","owner_pk":"{owner_pk}","timestamp":1700000000,"signature":"{signature}""#
    ))
}

// @internal
#[test]
fn a_purge_from_an_old_client_keeps_the_guardian_set() {
    let req: V2PurgeRequest = serde_json::from_str(&purge_json("")).unwrap();

    assert!(!req.erase_guardian_set);
}

// @internal
#[test]
fn a_purge_that_erases_the_guardian_set_says_so_on_the_wire() {
    let req: V2PurgeRequest =
        serde_json::from_str(&purge_json(r#","erase_guardian_set":true"#)).unwrap();
    assert!(req.erase_guardian_set);

    let json = serde_json::to_string(&req).unwrap();

    assert!(json.contains(r#""erase_guardian_set":true"#));
    let parsed: V2PurgeRequest = serde_json::from_str(&json).unwrap();
    assert!(parsed.erase_guardian_set);
}

// An old relay sees exactly the purge it always did.
// @internal
#[test]
fn a_purge_that_keeps_the_guardian_set_sends_no_new_field() {
    let req: V2PurgeRequest = serde_json::from_str(&purge_json("")).unwrap();

    let json = serde_json::to_string(&req).unwrap();

    assert!(!json.contains("erase_guardian_set"), "{json}");
}

// @internal
#[test]
fn a_proof_from_an_old_client_has_no_owner() {
    let req: V2RecoveryStoreRequest = serde_json::from_str(&recovery_store_json("")).unwrap();

    assert_eq!(req.owner_pk, None);
    assert_eq!(req.timestamp, None);
    assert_eq!(req.signature, None);
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("owner_pk"), "{json}");
    assert!(!json.contains("timestamp"), "{json}");
    assert!(!json.contains("signature"), "{json}");
}

// @internal
#[test]
fn an_owned_proof_carries_its_owner_timestamp_and_signature() {
    let json = owned_proof(&"ee".repeat(32), &"ff".repeat(64));

    let req: V2RecoveryStoreRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(req.owner_pk.as_deref(), Some("ee".repeat(32).as_str()));
    assert_eq!(req.timestamp, Some(1_700_000_000));
    assert_eq!(req.signature.as_deref(), Some("ff".repeat(64).as_str()));
}

// The owner fields share the size limits of the keys and signatures
// elsewhere in the protocol.
// @internal
#[test]
fn an_oversized_owner_key_or_signature_is_refused() {
    for (owner_pk, signature) in [
        ("e".repeat(65), "ff".repeat(64)),
        ("ee".repeat(32), "f".repeat(129)),
    ] {
        let err =
            serde_json::from_str::<V2RecoveryStoreRequest>(&owned_proof(&owner_pk, &signature))
                .unwrap_err();
        assert!(err.to_string().contains("too long"), "{err}");
    }
}

proptest! {
    // @internal
    #[test]
    fn the_new_fields_survive_a_round_trip(
        erase_guardian_set: bool,
        owner in proptest::option::of(("[0-9a-f]{64}", any::<u64>(), "[0-9a-f]{128}")),
    ) {
        let mut purge: V2PurgeRequest = serde_json::from_str(&purge_json("")).unwrap();
        purge.erase_guardian_set = erase_guardian_set;
        let mut proof: V2RecoveryStoreRequest =
            serde_json::from_str(&recovery_store_json("")).unwrap();
        proof.owner_pk = owner.as_ref().map(|(pk, _, _)| pk.clone());
        proof.timestamp = owner.as_ref().map(|(_, ts, _)| *ts);
        proof.signature = owner.as_ref().map(|(_, _, sig)| sig.clone());

        let purge_back: V2PurgeRequest =
            serde_json::from_str(&serde_json::to_string(&purge).unwrap()).unwrap();
        let proof_back: V2RecoveryStoreRequest =
            serde_json::from_str(&serde_json::to_string(&proof).unwrap()).unwrap();

        prop_assert_eq!(purge_back.erase_guardian_set, erase_guardian_set);
        prop_assert_eq!(proof_back.owner_pk, proof.owner_pk);
        prop_assert_eq!(proof_back.timestamp, proof.timestamp);
        prop_assert_eq!(proof_back.signature, proof.signature);
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::identity::Identity;
#[cfg(feature = "storage")]
use proptest::prelude::*;

fn registry() -> Vec<u8> {
    let identity = Identity::create("Alice", 0);
    RegistryBroadcast::new(
        &identity.initial_device_registry(),
        identity.signing_keypair(),
        0,
    )
    .to_json()
    .into_bytes()
}

fn push() -> Vec<u8> {
    let mut bytes = vec![PUSH_VERSION];
    bytes.extend_from_slice(&[7; NONCE_LEN]);
    bytes.extend_from_slice(&registry());
    bytes
}

fn ack(echo: bool) -> Vec<u8> {
    let mut bytes = vec![ACK_VERSION];
    bytes.extend_from_slice(&[7; NONCE_LEN]);
    bytes.extend_from_slice(&1u64.to_be_bytes());
    bytes.push(u8::from(echo));
    if echo {
        bytes.extend_from_slice(&registry());
    }
    bytes
}

// @internal
#[test]
fn valid_registry_controls_are_admitted_without_a_storage_dependency() {
    for payload in [push(), ack(false), ack(true)] {
        assert!(is_registry_control(&payload));
    }
}

// @internal
#[test]
fn private_tags_and_malformed_registry_framing_are_refused() {
    let mut private = push();
    private[0] = 0x04;
    let mut trailing = ack(false);
    trailing.push(0);
    let mut invalid_flag = ack(true);
    invalid_flag[ACK_PREFIX_LEN] = 2;
    let mut invalid_json = push();
    invalid_json.push(b'x');
    for payload in [
        Vec::new(),
        private,
        trailing,
        invalid_flag,
        invalid_json,
        vec![PUSH_VERSION; NONCE_LEN],
        vec![ACK_VERSION; ACK_PREFIX_LEN],
        vec![PUSH_VERSION; MAX_REGISTRY_BROADCAST_JSON_BYTES + NONCE_LEN + 2],
    ] {
        assert!(
            !is_registry_control(&payload),
            "malformed frame of {} bytes",
            payload.len()
        );
    }
}

// @internal
#[cfg(feature = "storage")]
#[test]
fn admission_matches_the_canonical_storage_wire_decoder() {
    use crate::sync::delta::VersionedPayload;

    for valid in [push(), ack(false), ack(true)] {
        let mut candidates = vec![valid.clone()];
        candidates.extend((0..valid.len()).map(|length| valid[..length].to_vec()));
        for flag in [0, 1, 2, 0xff] {
            let mut changed = valid.clone();
            changed[ACK_PREFIX_LEN.min(valid.len() - 1)] = flag;
            candidates.push(changed);
        }
        for payload in candidates {
            let decoded = matches!(
                VersionedPayload::decode(&payload),
                Ok(VersionedPayload::RegistryPush(_)) | Ok(VersionedPayload::RegistryAck(_))
            );
            assert_eq!(is_registry_control(&payload), decoded);
        }
    }
}

#[cfg(feature = "storage")]
proptest! {
    // @internal
    #[test]
    fn arbitrary_admission_matches_typed_decoding(
        payload in prop::collection::vec(any::<u8>(), 0..8235)
    ) {
        use crate::sync::delta::VersionedPayload;
        let decoded = matches!(VersionedPayload::decode(&payload),
            Ok(VersionedPayload::RegistryPush(_)) | Ok(VersionedPayload::RegistryAck(_)));
        prop_assert_eq!(is_registry_control(&payload), decoded);
    }
}

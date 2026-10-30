// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::contact::Contact;
use crate::identity::RegistryBroadcast;

fn signed_peer_registry(pair: &Pair, retain_older: bool) -> (String, String, u64) {
    let peer = Identity::create("Peer", NOW);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Peer"),
        SymmetricKey::generate(),
        NOW,
    );
    let contact_id = contact.id().to_string();
    pair.receiver.add_contact(contact).unwrap();
    let mut registry = peer.initial_device_registry();
    if retain_older {
        let older = RegistryBroadcast::new(&registry, peer.signing_keypair(), NOW);
        pair.receiver
            .storage()
            .device()
            .save_contact_device_registry(&contact_id, &older, peer.signing_public_key(), u64::MAX)
            .unwrap();
    }
    let sibling = Identity::from_device_link(
        *peer.master_seed(),
        "Peer".into(),
        1,
        "Peer tablet".into(),
        NOW,
    );
    registry
        .add_device(
            sibling.device_info().to_registered(peer.master_seed()),
            peer.signing_keypair(),
        )
        .unwrap();
    let broadcast = RegistryBroadcast::new(&registry, peer.signing_keypair(), NOW);
    (contact_id, broadcast.to_json(), broadcast.version())
}

#[test]
fn own_device_order_registry_storage_failure_is_retryable_without_consuming_sequence() {
    for retain_older in [false, true] {
        let pair = Pair::new();
        let (contact_id, registry_json, version) = signed_peer_registry(&pair, retain_older);
        let mut sender = pair.sender();
        sender
            .record_local_change(SyncItem::ContactRegistryReceived {
                contact_id: contact_id.clone(),
                registry_json: registry_json.clone(),
                version,
                timestamp: NOW,
            })
            .unwrap();
        let envelope: SyncEnvelope =
            serde_json::from_slice(&sender.build_outbound_envelopes(&pair.sender).unwrap()[0])
                .unwrap();
        pair.receiver.storage().connection().execute_batch(
            "CREATE TRIGGER reject_peer_registry BEFORE INSERT ON contact_device_registries BEGIN SELECT RAISE(ABORT, 'injected registry write failure'); END;"
        ).unwrap();
        let direct = pair
            .receiver
            .apply_contact_registry_received(&contact_id, &registry_json);
        assert!(
            matches!(direct, Err(VauchiError::Storage(_))),
            "actual write errors must retain their storage type"
        );
        let public = pair
            .receiver
            .apply_sync_items(vec![SyncItem::ContactRegistryReceived {
                contact_id: contact_id.clone(),
                registry_json,
                version,
                timestamp: NOW,
            }])
            .unwrap();
        assert_eq!(
            public, 0,
            "public best-effort application keeps its existing skip semantics"
        );
        let failed = pair
            .receiver
            .apply_device_sync_blob(pair.receiver.identity().unwrap(), &envelope.ciphertext);
        pair.receiver
            .storage()
            .connection()
            .execute_batch("DROP TRIGGER reject_peer_registry;")
            .unwrap();
        let retried = pair
            .receiver
            .apply_device_sync_blob(pair.receiver.identity().unwrap(), &envelope.ciphertext)
            .unwrap();
        assert!(
            failed.is_err(),
            "a real registry write failure must remain retryable: {failed:?}"
        );
        assert_eq!(
            retried, 1,
            "failed registry persistence must not consume its authenticated sequence"
        );
        let stored = pair
            .receiver
            .storage()
            .device()
            .load_contact_device_registry(&contact_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.version(), version);
        assert_eq!(stored.active_device_count(), 2);
        assert_eq!(pair.apply(&envelope.ciphertext), 0);
    }
}

#[test]
fn own_device_order_invalid_signed_registry_does_not_block_independent_item() {
    let pair = Pair::new();
    let (contact_id, registry_json, version) = signed_peer_registry(&pair, false);
    let mut invalid: serde_json::Value = serde_json::from_str(&registry_json).unwrap();
    // The version is signed; retaining the original signature must reject this.
    invalid["version"] = serde_json::json!(version + 1);
    let mut sender = pair.sender();
    sender
        .record_local_change(SyncItem::ContactRegistryReceived {
            contact_id: contact_id.clone(),
            registry_json: invalid.to_string(),
            version: version + 1,
            timestamp: NOW,
        })
        .unwrap();
    sender
        .record_local_change(SyncItem::CardUpdated {
            field_label: "Email".into(),
            new_value: "owner@example.test".into(),
            timestamp: NOW,
        })
        .unwrap();
    let envelope: SyncEnvelope =
        serde_json::from_slice(&sender.build_outbound_envelopes(&pair.sender).unwrap()[0]).unwrap();
    assert_eq!(
        pair.apply(&envelope.ciphertext),
        1,
        "invalid signed registry remains a skipped domain rejection"
    );
    assert_eq!(pair.value("Email"), "owner@example.test");
    assert!(
        pair.receiver
            .storage()
            .device()
            .load_contact_device_registry(&contact_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(pair.apply(&envelope.ciphertext), 0);
}

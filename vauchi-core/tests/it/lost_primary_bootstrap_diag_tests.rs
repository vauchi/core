// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic in-process reproduction of the F4 six-device lost-primary
//! bootstrap, routing genesis handshake push/ack through the real
//! `process_received_blobs` receive path (recipient-keyed mailbox tokens,
//! consume-once). Instruments each step so the ACTUAL stall is named — the
//! OHTTP-split e2e cert (`multi_device_sync.rs:780`) fails identically across
//! three core versions and cannot run locally (OHTTP 502).

use vauchi_core::SymmetricKey;
use vauchi_core::api::Vauchi;
use vauchi_core::api::vauchi::process_received_blobs;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::network::mailbox_token::{
    compute_device_mailbox_token, compute_mailbox_token, current_day_epoch, token_hex,
};
use vauchi_core::sync::registry_activation::ActivationState;

struct Party {
    devices: Vec<Vauchi>,
    seed: [u8; 32],
    signing_public_key: [u8; 32],
    card: ContactCard,
}

fn make_party(seed: [u8; 32], name: &str, email: &str, n: u32) -> Party {
    let field = ContactField::new(FieldType::Email, "Email", email, 1);
    let field_id = field.id().to_string();
    let mut devices = Vec::new();
    for index in 0..n {
        let mut device = Vauchi::in_memory().unwrap();
        let identity =
            Identity::from_device_link(seed, name.into(), index, format!("{name} {index}"), 1);
        device.set_identity(identity).unwrap();
        device.add_own_field(field.clone()).unwrap();
        device.set_own_field_public(&field_id).unwrap();
        devices.push(device);
    }
    let signing_public_key = *devices[0].identity().unwrap().signing_public_key();
    let card = devices[0].own_card().unwrap().unwrap();
    Party {
        devices,
        seed,
        signing_public_key,
        card,
    }
}

/// Broadcast covering only the first `count` devices of the party — models a
/// peer that has learned a partial device registry.
fn broadcast_for(party: &Party, count: usize, now: u64) -> RegistryBroadcast {
    let keypair = party.devices[0].identity().unwrap().signing_keypair();
    let mut registry = DeviceRegistry::new(
        party.devices[0]
            .identity()
            .unwrap()
            .device_info()
            .to_registered(&party.seed),
        keypair,
    );
    for device in party.devices.iter().take(count).skip(1) {
        registry
            .add_device(
                device
                    .identity()
                    .unwrap()
                    .device_info()
                    .to_registered(&party.seed),
                keypair,
            )
            .unwrap();
    }
    RegistryBroadcast::new(&registry, keypair, now)
}

/// The deposit mailbox token the send phase would use for a pending update:
/// device-scoped when it targets a specific recipient device, else the
/// recipient's identity mailbox (send_phase.rs:251-259).
fn deposit_token_hex(
    shared: &[u8; 32],
    recipient_pk: &[u8; 32],
    target_device_id: Option<[u8; 32]>,
    day: u64,
) -> String {
    match target_device_id {
        Some(dev) => token_hex(&compute_device_mailbox_token(
            shared,
            recipient_pk,
            &dev,
            day,
        )),
        None => token_hex(&compute_mailbox_token(shared, recipient_pk, day)),
    }
}

fn activation_state(device: &Vauchi, contact_id: &str) -> ActivationState {
    device
        .storage()
        .registry_activation()
        .load_activation(contact_id)
        .unwrap()
        .map(|t| t.state())
        .unwrap_or(ActivationState::Dormant)
}

// RED reproduction of the e2e lost-primary cert (multi_device_sync.rs:780).
// STEP 1-4 PASS: the handshake bootstrap converges (sender-token fix). STEP 5
// FAILS and pins the real tail bug: A2's post-Active card fan-out hits
// `NoSendingChain` (A2 is responder-side) and `repropagate_to_contact` aborts,
// so the card reaches no Bob device. Un-ignore when the genesis-card fan-out
// fallback + receive handler land.
// @scenario: multi_device_sync :: Lost exchanging device no longer orphans the relationship
// @internal
#[test]
fn lost_primary_cold_bootstrap_pins_the_stall() {
    let relationship = SymmetricKey::from_bytes([42u8; 32]);
    let alice = make_party([10u8; 32], "Alice", "alice@old", 3);
    let bob = make_party([20u8; 32], "Bob", "bob@old", 3);
    let now = alice.devices[0].storage().clock().unix_seconds();
    let day = current_day_epoch(now);

    // Bob learned Alice via A1<->B1 exchange only: Bob knows A1 alone. Alice's
    // siblings owner-synced the whole Bob registry from the now-lost A1.
    let bob_full = broadcast_for(&bob, 3, now);
    let alice_a1_only = broadcast_for(&alice, 1, now);

    let alice_bob_contact_id = {
        let mut id = String::new();
        for d in &alice.devices {
            let c = Contact::from_exchange(
                bob.signing_public_key,
                bob.card.clone(),
                relationship.clone(),
                now,
            );
            id = c.id().to_string();
            d.add_contact(c).unwrap();
            d.storage()
                .device()
                .save_contact_device_registry(&id, &bob_full, &bob.signing_public_key, 60)
                .unwrap();
        }
        id
    };
    for d in &bob.devices {
        let c = Contact::from_exchange(
            alice.signing_public_key,
            alice.card.clone(),
            relationship.clone(),
            now,
        );
        let cid = c.id().to_string();
        d.add_contact(c).unwrap();
        d.storage()
            .device()
            .save_contact_device_registry(&cid, &alice_a1_only, &alice.signing_public_key, 60)
            .unwrap();
    }

    // A1 (index 0) is permanently lost; the surviving sibling A2 (index 1)
    // must bootstrap the handshake with Bob from cold.
    let a2 = &alice.devices[1];

    // ─── STEP 1: A2 queues a bootstrap push ───────────────────────────────
    let queued = a2.queue_registry_pushes().unwrap();
    eprintln!("STEP1: A2.queue_registry_pushes() -> {queued}");
    assert_eq!(queued, 1, "A2 must queue exactly one bootstrap push");
    let pending = a2.storage().pending().get_all_pending_updates().unwrap();
    assert_eq!(pending.len(), 1, "one pending push blob");
    let push = pending[0].clone();
    eprintln!(
        "STEP1: push type={:?} target_device={:?} payload_len={}",
        push.update_type,
        push.target_device_id.map(|d| hex::encode(&d[..4])),
        push.payload.len()
    );
    let push_token = deposit_token_hex(
        relationship.as_bytes(),
        &bob.signing_public_key,
        push.target_device_id,
        day,
    );

    // ─── STEP 2: deliver A2's push to each Bob device via the real receive ─
    let mut reply = None;
    let mut recv_idx = None;
    for (i, bd) in bob.devices.iter().enumerate() {
        let contacts = bd.storage().contacts().list_contacts().unwrap();
        let outcomes = process_received_blobs(
            bd.identity().unwrap(),
            bd.storage(),
            &contacts,
            vec![("push".to_string(), push_token.clone(), push.payload.clone())],
        );
        let o = &outcomes[0];
        eprintln!(
            "STEP2: B{} resolved={} decrypted={} reject={:?} reply={}",
            i + 1,
            o.token_resolved,
            o.decrypted,
            o.reject_reason,
            o.registry_reply.is_some()
        );
        if let Some(r) = &o.registry_reply {
            reply = Some(r.clone());
            recv_idx = Some(i);
        }
    }
    let reply = reply.expect("STALL@2: no Bob device could open A2's cold-start push");
    let recv_idx = recv_idx.unwrap();
    eprintln!("STEP2: B{} produced an ack reply", recv_idx + 1);

    // ─── STEP 3: that Bob device queues the ack; deliver it back to A2 ─────
    bob.devices[recv_idx].queue_registry_ack(&reply).unwrap();
    let bob_pending = bob.devices[recv_idx]
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    eprintln!(
        "STEP3: B{} queued {} ack blob(s)",
        recv_idx + 1,
        bob_pending.len()
    );
    assert!(!bob_pending.is_empty(), "STALL@3: Bob queued no ack");
    let ack = bob_pending[0].clone();
    let ack_token = deposit_token_hex(
        relationship.as_bytes(),
        &alice.signing_public_key,
        ack.target_device_id,
        day,
    );
    eprintln!(
        "STEP3: ack target_device={:?}",
        ack.target_device_id.map(|d| hex::encode(&d[..4]))
    );

    for (i, ad) in alice.devices.iter().enumerate().skip(1) {
        let contacts = ad.storage().contacts().list_contacts().unwrap();
        let outcomes = process_received_blobs(
            ad.identity().unwrap(),
            ad.storage(),
            &contacts,
            vec![("ack".to_string(), ack_token.clone(), ack.payload.clone())],
        );
        let o = &outcomes[0];
        eprintln!(
            "STEP3: A{} resolved={} decrypted={} reject={:?} state={:?}",
            i + 1,
            o.token_resolved,
            o.decrypted,
            o.reject_reason,
            activation_state(ad, &alice_bob_contact_id)
        );
    }

    // ─── STEP 4: A2 must now be Active for the Bob contact ────────────────
    let a2_state = activation_state(&alice.devices[1], &alice_bob_contact_id);
    eprintln!("STEP4: A2 activation state = {a2_state:?}");
    assert_eq!(
        a2_state,
        ActivationState::Active,
        "STALL@4: A2 did not reach Active after the ack round-trip"
    );
    eprintln!("REACHED: handshake bootstrap complete (A2 Active).");

    // ─── STEP 5: A2 publishes a new field and fans it out to Bob's fleet ──
    let bob_alice_contact_id = hex::encode(alice.signing_public_key);
    let phone = ContactField::new(FieldType::Phone, "Phone", "+12025550811", 1);
    let phone_id = phone.id().to_string();
    alice.devices[1].add_own_field(phone).unwrap();
    // Drain the handshake push so only card deltas remain in pending.
    for u in alice.devices[1]
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
    {
        let _ = alice.devices[1].storage().pending().mark_update_sent(&u.id);
    }
    alice.devices[1]
        .set_field_public_and_repropagate(&alice_bob_contact_id, &phone_id)
        .unwrap();

    let card_pending: Vec<_> = alice.devices[1]
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .into_iter()
        .filter(|u| u.update_type != "registry_handshake")
        .collect();
    eprintln!(
        "STEP5: A2 repropagate -> {} card blob(s), targets={:?}",
        card_pending.len(),
        card_pending
            .iter()
            .map(|u| u.target_device_id.map(|d| hex::encode(&d[..4])))
            .collect::<Vec<_>>()
    );

    for u in &card_pending {
        let tok = deposit_token_hex(
            relationship.as_bytes(),
            &bob.signing_public_key,
            u.target_device_id,
            day,
        );
        for (i, bd) in bob.devices.iter().enumerate() {
            let contacts = bd.storage().contacts().list_contacts().unwrap();
            let out = process_received_blobs(
                bd.identity().unwrap(),
                bd.storage(),
                &contacts,
                vec![("card".to_string(), tok.clone(), u.payload.clone())],
            );
            let o = &out[0];
            if o.token_resolved {
                eprintln!(
                    "STEP5: blob(target={:?}) -> B{} resolved={} decrypted={} reject={:?}",
                    u.target_device_id.map(|d| hex::encode(&d[..4])),
                    i + 1,
                    o.token_resolved,
                    o.decrypted,
                    o.reject_reason
                );
            }
        }
    }

    let mut missing = Vec::new();
    for (i, bd) in bob.devices.iter().enumerate() {
        let c = bd.get_contact(&bob_alice_contact_id).unwrap().unwrap();
        let has = c
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "+12025550811");
        eprintln!("STEP5: B{} has Alice's new phone = {has}", i + 1);
        if !has {
            missing.push(format!("B{}", i + 1));
        }
    }
    assert!(
        missing.is_empty(),
        "STALL@5 (reproduces cert): A2's field did not reach {missing:?}"
    );
    eprintln!("REACHED: full six-device lost-primary convergence.");

    // ─── STEP 6: adversarial — the genesis-card path is not a bypass ──────
    // A2 is responder-side for the whole fleet (identity ordering), so every
    // card above was genesis-sealed. Prove shared_key possession is parser
    // admission, not authority: a replay and a tampered envelope are rejected.
    let card = &card_pending[0];
    let target_dev = card.target_device_id.expect("device-scoped genesis card");
    let tok = deposit_token_hex(
        relationship.as_bytes(),
        &bob.signing_public_key,
        card.target_device_id,
        day,
    );
    let bd = bob
        .devices
        .iter()
        .find(|d| d.identity().unwrap().device_id() == &target_dev)
        .expect("the target Bob device");
    let contacts = bd.storage().contacts().list_contacts().unwrap();

    // Replay of the already-applied blob: the delta nonce is burned, so it must
    // not re-apply.
    let replay = process_received_blobs(
        bd.identity().unwrap(),
        bd.storage(),
        &contacts,
        vec![("replay".to_string(), tok.clone(), card.payload.clone())],
    );
    eprintln!(
        "STEP6: replay -> resolved={} decrypted={} reject={:?}",
        replay[0].token_resolved, replay[0].decrypted, replay[0].reject_reason
    );
    assert!(
        !replay[0].decrypted,
        "STEP6: a replayed genesis card must be rejected"
    );

    // Tampered envelope: flipping a byte breaks the genesis seal, so it fails to
    // open and applies nothing.
    let mut tampered_payload = card.payload.clone();
    if let Some(b) = tampered_payload.last_mut() {
        *b ^= 0xFF;
    }
    let tampered = process_received_blobs(
        bd.identity().unwrap(),
        bd.storage(),
        &contacts,
        vec![("tampered".to_string(), tok, tampered_payload)],
    );
    eprintln!(
        "STEP6: tampered -> decrypted={} reject={:?}",
        tampered[0].decrypted, tampered[0].reject_reason
    );
    assert!(
        !tampered[0].decrypted,
        "STEP6: a tampered genesis card must be rejected"
    );
    eprintln!("REACHED: adversarial genesis-card checks pass.");
}

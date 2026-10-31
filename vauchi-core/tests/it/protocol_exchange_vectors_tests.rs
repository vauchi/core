// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Known-answer vectors for the two exchange transcripts
//! (vauchi/private#119): the mutual QR key agreement with its reciprocity
//! tokens and confirmation escrow, and one link exchange from the URL to
//! the shared key. They share the fixture, loader and regeneration of
//! `protocol_test_vectors_tests.rs`.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vauchi_core::contact::reciprocity_tokens::derive_confirmation_tokens;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{HKDF, SymmetricKey, X3DHKeyPair, decrypt};
use vauchi_core::exchange::confirmation_escrow::ConfirmationEscrowKeys;
use vauchi_core::exchange::escrow::{EscrowKeys, EscrowRole};
use vauchi_core::exchange::key_order::is_initiator;
use vauchi_core::exchange::link_mode::{
    LinkExchangeBinding, derive_link_shared_key, initiator_generate_named, parse_link_card_payload,
    parse_link_url, serialize_link_card_payload,
};
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, ExchangeState, MockProximityVerifier};
use vauchi_core::identity::Identity;

use super::protocol_test_vectors_tests::{hex32, load, seed};

const QR_SYMMETRIC_INFO_PREFIX: &str = "vauchi-x3dh-symmetric-v2";
const RECIPROCITY_CONFIRM_INFO: &str = "vauchi-reciprocity-confirm-v1";
const CONFIRM_GATE_INFO: &str = "vauchi-confirm-escrow-gate-v1";
const CONFIRM_SLOT_INIT_INFO: &str = "vauchi-confirm-escrow-slot-init-v1";
const CONFIRM_SLOT_RESP_INFO: &str = "vauchi-confirm-escrow-slot-resp-v1";
const ESCROW_GATE_INFO: &str = "vauchi-escrow-gate-v1";
const ESCROW_SLOT_INIT_INFO: &str = "vauchi-escrow-slot-init-v1";
const ESCROW_SLOT_RESP_INFO: &str = "vauchi-escrow-slot-resp-v1";
const ESCROW_CARD_KEY_INFO: &str = "vauchi-escrow-card-key-v1";
const LINK_BOOTSTRAP_DOMAIN: &str = "vauchi-link-bootstrap-v3";
const LINK_SHARED_KEY_INFO: &str = "vauchi-link-shared-key-v1";
const VECTOR_NOW: u64 = 1_793_100_000;

/// The mutual QR exchange: both sides hold fresh X25519 keys, the key is
/// HKDF over their DH with both identity keys and both ephemeral keys
/// bound into the info, and the reciprocity tokens and confirmation escrow
/// slots derive from the raw DH output (In-Person Exchange A4, A5).
#[derive(Serialize, Deserialize)]
pub(crate) struct QrSymmetricVector {
    info_prefix: String,
    a_master_seed: String,
    a_identity_public: String,
    b_master_seed: String,
    b_identity_public: String,
    a_ephemeral_secret: String,
    a_ephemeral_public: String,
    b_ephemeral_secret: String,
    b_ephemeral_public: String,
    dh: String,
    info: String,
    shared_key: String,
    initiator: String,
    confirmation_info: String,
    a_confirmation_token: String,
    b_confirmation_token: String,
    confirmation_gate_info: String,
    confirmation_slot_init_info: String,
    confirmation_slot_resp_info: String,
    confirmation_gate_hash: String,
    confirmation_slot_init: String,
    confirmation_slot_resp: String,
}

/// One link exchange end to end: the link, the three nonce-derived slots,
/// the escrow gate, slots and card key from the ephemeral DH, the signed v2
/// card payload and the shared key the two fresh X3DH keys give (Link and
/// Relay-Mediated Exchange A1 to A10).
#[derive(Serialize, Deserialize)]
pub(crate) struct LinkExchangeVector {
    url: String,
    nonce: String,
    initiator_secret: String,
    initiator_public: String,
    responder_secret: String,
    responder_public: String,
    handshake_gate: String,
    presence_slot: String,
    epk_slot: String,
    dh: String,
    gate_info: String,
    slot_init_info: String,
    slot_resp_info: String,
    card_key_info: String,
    escrow_gate: String,
    escrow_slot_initiator: String,
    escrow_slot_responder: String,
    card_key: String,
    bootstrap_domain: String,
    initiator_identity_seed: String,
    initiator_identity_public: String,
    initiator_x3dh_secret: String,
    initiator_x3dh_public: String,
    responder_x3dh_secret: String,
    responder_x3dh_public: String,
    relay_url: String,
    card_id: String,
    card_display_name: String,
    initiator_payload_v3: String,
    bootstrap_signature: String,
    link_shared_key_info: String,
    link_shared_key: String,
}

// ── Mutual QR exchange ────────────────────────────────────────────────

fn vector_identity(seed_byte: u8, name: &str) -> Identity {
    Identity::from_device_link(
        seed(seed_byte),
        name.to_string(),
        0,
        "vector-device".to_string(),
        VECTOR_NOW,
    )
}

/// Drives one side of the mutual QR exchange through the shipped session
/// up to the key agreement and returns the shared key and the two
/// reciprocity tokens the session derived.
fn qr_side(
    identity: Identity,
    ephemeral: X3DHKeyPair,
    peer_qr: &vauchi_core::exchange::ExchangeQR,
) -> ([u8; 32], [u8; 32], [u8; 32]) {
    let mut session = ExchangeSession::new_qr_with_x3dh(
        identity,
        ContactCard::new("Vector"),
        MockProximityVerifier::success(),
        ephemeral,
        vauchi_core::clock::SystemClock::shared(),
    );
    session.apply(ExchangeEvent::StartQR).expect("start");
    session
        .apply(ExchangeEvent::ProcessQR(peer_qr.clone()))
        .expect("scan");
    session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .expect("they scanned");
    session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .expect("agree");
    let shared = match session.state() {
        ExchangeState::AwaitingCardExchange { shared_key, .. } => *shared_key.as_bytes(),
        other => panic!("expected AwaitingCardExchange, got {other:?}"),
    };
    (
        shared,
        *session.our_confirmation_token().expect("our token"),
        *session.expected_their_token().expect("their token"),
    )
}

/// Both sessions, each shown the other's QR. The QR carries a random token
/// and audio challenge, so the QR bytes themselves are not reproducible;
/// the key and the tokens depend only on the four public keys and the DH.
fn qr_exchange(a_seed: u8, b_seed: u8, a_eph: u8, b_eph: u8) -> ([u8; 32], [u8; 32], [u8; 32]) {
    // Identity is not Clone, so each session gets its own copy from the seed.
    let qr_of = |identity: Identity, eph: X3DHKeyPair| {
        let mut session = ExchangeSession::new_qr_with_x3dh(
            identity,
            ContactCard::new("Vector"),
            MockProximityVerifier::success(),
            eph,
            vauchi_core::clock::SystemClock::shared(),
        );
        session.apply(ExchangeEvent::StartQR).expect("start");
        session.qr().expect("qr").clone()
    };
    let qr_a = qr_of(
        vector_identity(a_seed, "Vector A"),
        X3DHKeyPair::from_bytes(seed(a_eph)),
    );
    let qr_b = qr_of(
        vector_identity(b_seed, "Vector B"),
        X3DHKeyPair::from_bytes(seed(b_eph)),
    );
    let (key_a, a_token, a_expects) = qr_side(
        vector_identity(a_seed, "Vector A"),
        X3DHKeyPair::from_bytes(seed(a_eph)),
        &qr_b,
    );
    let (key_b, b_token, b_expects) = qr_side(
        vector_identity(b_seed, "Vector B"),
        X3DHKeyPair::from_bytes(seed(b_eph)),
        &qr_a,
    );
    assert_eq!(key_a, key_b, "both sides derive the same key");
    assert_eq!(a_token, b_expects, "A's token is what B expects");
    assert_eq!(b_token, a_expects, "B's token is what A expects");
    (key_a, a_token, b_token)
}

pub(crate) fn compute_qr_symmetric() -> QrSymmetricVector {
    let (a_seed, b_seed, a_eph_seed, b_eph_seed) = (0x60u8, 0x61u8, 0x62u8, 0x63u8);
    let a = vector_identity(a_seed, "Vector A");
    let b = vector_identity(b_seed, "Vector B");
    let a_eph = X3DHKeyPair::from_bytes(seed(a_eph_seed));
    let b_eph = X3DHKeyPair::from_bytes(seed(b_eph_seed));
    let dh = a_eph.diffie_hellman(b_eph.public_key()).expect("dh");
    let (id_lo, id_hi) = vauchi_core::exchange::key_order::sorted_pair(
        a.signing_public_key().as_slice(),
        b.signing_public_key().as_slice(),
    );
    let (eph_lo, eph_hi) = vauchi_core::exchange::key_order::sorted_pair(
        a_eph.public_key().as_slice(),
        b_eph.public_key().as_slice(),
    );
    let mut info = QR_SYMMETRIC_INFO_PREFIX.as_bytes().to_vec();
    info.extend_from_slice(id_lo);
    info.extend_from_slice(id_hi);
    info.extend_from_slice(eph_lo);
    info.extend_from_slice(eph_hi);
    let (shared_key, a_token, b_token) = qr_exchange(a_seed, b_seed, a_eph_seed, b_eph_seed);
    let a_is_initiator = is_initiator(a.signing_public_key(), b.signing_public_key());
    let confirm = ConfirmationEscrowKeys::derive(&dh[..], EscrowRole::Initiator);
    QrSymmetricVector {
        info_prefix: QR_SYMMETRIC_INFO_PREFIX.to_string(),
        a_master_seed: hex::encode(seed(a_seed)),
        a_identity_public: hex::encode(a.signing_public_key()),
        b_master_seed: hex::encode(seed(b_seed)),
        b_identity_public: hex::encode(b.signing_public_key()),
        a_ephemeral_secret: hex::encode(*a_eph.secret_bytes()),
        a_ephemeral_public: hex::encode(a_eph.public_key()),
        b_ephemeral_secret: hex::encode(*b_eph.secret_bytes()),
        b_ephemeral_public: hex::encode(b_eph.public_key()),
        dh: hex::encode(*dh),
        info: hex::encode(&info),
        shared_key: hex::encode(shared_key),
        initiator: if a_is_initiator { "a" } else { "b" }.to_string(),
        confirmation_info: RECIPROCITY_CONFIRM_INFO.to_string(),
        a_confirmation_token: hex::encode(a_token),
        b_confirmation_token: hex::encode(b_token),
        confirmation_gate_info: CONFIRM_GATE_INFO.to_string(),
        confirmation_slot_init_info: CONFIRM_SLOT_INIT_INFO.to_string(),
        confirmation_slot_resp_info: CONFIRM_SLOT_RESP_INFO.to_string(),
        confirmation_gate_hash: confirm.gate_hash.clone(),
        confirmation_slot_init: confirm.our_slot.clone(),
        confirmation_slot_resp: confirm.their_slot.clone(),
    }
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn qr_symmetric_vector_matches_the_session_and_its_formulas() {
    let v = load().qr_symmetric;
    let (shared_key, a_token, b_token) = qr_exchange(0x60, 0x61, 0x62, 0x63);
    assert_eq!(hex::encode(shared_key), v.shared_key, "the session's key");
    assert_eq!(hex::encode(a_token), v.a_confirmation_token);
    assert_eq!(hex::encode(b_token), v.b_confirmation_token);

    // K = HKDF(salt = none, ikm = DH, prefix || sorted ids || sorted ephs).
    let dh = hex::decode(&v.dh).expect("dh");
    let info = hex::decode(&v.info).expect("info");
    assert!(info.starts_with(v.info_prefix.as_bytes()));
    assert_eq!(info.len(), v.info_prefix.len() + 4 * 32);
    assert_eq!(
        hex::encode(*HKDF::derive_key(None, &dh, &info)),
        v.shared_key
    );

    // Tokens: HKDF(DH, confirmation_info || own identity key), from the raw DH.
    let (a_by_hand, b_by_hand) = derive_confirmation_tokens(
        &dh,
        &hex32(&v.a_identity_public),
        &hex32(&v.b_identity_public),
    );
    assert_eq!(hex::encode(a_by_hand), v.a_confirmation_token);
    assert_eq!(hex::encode(b_by_hand), v.b_confirmation_token);
    let mut a_info = v.confirmation_info.as_bytes().to_vec();
    a_info.extend_from_slice(&hex32(&v.a_identity_public));
    assert_eq!(
        hex::encode(*HKDF::derive_key(None, &dh, &a_info)),
        v.a_confirmation_token
    );

    // Confirmation escrow: hex(SHA-256(HKDF(DH, info))) for gate and slots.
    let confirm = ConfirmationEscrowKeys::derive(&dh, EscrowRole::Initiator);
    assert_eq!(confirm.gate_hash, v.confirmation_gate_hash);
    assert_eq!(confirm.our_slot, v.confirmation_slot_init);
    assert_eq!(confirm.their_slot, v.confirmation_slot_resp);
    let gate_by_hand = hex::encode(Sha256::digest(
        HKDF::derive_key(None, &dh, v.confirmation_gate_info.as_bytes()).as_ref(),
    ));
    assert_eq!(gate_by_hand, v.confirmation_gate_hash);
    let expected_initiator =
        if is_initiator(&hex32(&v.a_identity_public), &hex32(&v.b_identity_public)) {
            "a"
        } else {
            "b"
        };
    assert_eq!(v.initiator, expected_initiator);
}

// ── Link exchange ─────────────────────────────────────────────────────

fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hex::encode(hasher.finalize())
}

/// A card whose id is fixed, so the signed payload is reproducible; the
/// shipped constructor draws a random id.
fn vector_card() -> ContactCard {
    let mut value = serde_json::to_value(ContactCard::new("Vector")).expect("card json");
    value["id"] = serde_json::Value::String("00112233445566778899aabbccddeeff".into());
    serde_json::from_value(value).expect("card from json")
}

pub(crate) fn compute_link_exchange() -> LinkExchangeVector {
    let nonce = seed(0x70);
    let initiator = X3DHKeyPair::from_bytes(seed(0x71));
    let responder = X3DHKeyPair::from_bytes(seed(0x72));
    let url = format!(
        "https://vauchi.app/x#pk={}&n={}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(initiator.public_key()),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce),
    );
    let dh = responder
        .diffie_hellman(initiator.public_key())
        .expect("escrow dh");
    let keys = EscrowKeys::derive(&dh[..], EscrowRole::Initiator);
    let card_key = HKDF::derive_key(None, &dh[..], ESCROW_CARD_KEY_INFO.as_bytes());

    let identity = vector_identity(0x73, "Vector Link");
    let x3dh_i = X3DHKeyPair::from_bytes(seed(0x74));
    let x3dh_r = X3DHKeyPair::from_bytes(seed(0x75));
    let relay_url = "https://relay.vauchi.app";
    let card = vector_card();
    let binding = LinkExchangeBinding {
        nonce,
        initiator_epk: *initiator.public_key(),
        responder_epk: *responder.public_key(),
    };
    let payload = serialize_link_card_payload(
        identity.signing_keypair(),
        x3dh_i.public_key(),
        relay_url,
        &card,
        &binding,
    );
    let signature = payload[1..65].to_vec();
    let link_shared_key = derive_link_shared_key(&x3dh_i, x3dh_r.public_key()).expect("shared");

    LinkExchangeVector {
        url,
        nonce: hex::encode(nonce),
        initiator_secret: hex::encode(*initiator.secret_bytes()),
        initiator_public: hex::encode(initiator.public_key()),
        responder_secret: hex::encode(*responder.secret_bytes()),
        responder_public: hex::encode(responder.public_key()),
        handshake_gate: sha256_hex(&[&nonce, b"handshake"]),
        presence_slot: sha256_hex(&[&nonce, b"initiator_presence"]),
        epk_slot: sha256_hex(&[&nonce, b"epk"]),
        dh: hex::encode(*dh),
        gate_info: ESCROW_GATE_INFO.to_string(),
        slot_init_info: ESCROW_SLOT_INIT_INFO.to_string(),
        slot_resp_info: ESCROW_SLOT_RESP_INFO.to_string(),
        card_key_info: ESCROW_CARD_KEY_INFO.to_string(),
        escrow_gate: keys.gate_hash.clone(),
        escrow_slot_initiator: keys.our_slot.clone(),
        escrow_slot_responder: keys.their_slot.clone(),
        card_key: hex::encode(*card_key),
        bootstrap_domain: LINK_BOOTSTRAP_DOMAIN.to_string(),
        initiator_identity_seed: hex::encode(seed(0x73)),
        initiator_identity_public: hex::encode(identity.signing_public_key()),
        initiator_x3dh_secret: hex::encode(*x3dh_i.secret_bytes()),
        initiator_x3dh_public: hex::encode(x3dh_i.public_key()),
        responder_x3dh_secret: hex::encode(*x3dh_r.secret_bytes()),
        responder_x3dh_public: hex::encode(x3dh_r.public_key()),
        relay_url: relay_url.to_string(),
        card_id: card.id().to_string(),
        card_display_name: "Vector".to_string(),
        initiator_payload_v3: hex::encode(&payload),
        bootstrap_signature: hex::encode(signature),
        link_shared_key_info: LINK_SHARED_KEY_INFO.to_string(),
        link_shared_key: hex::encode(link_shared_key.as_bytes()),
    }
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn link_exchange_vector_matches_the_url_slots_escrow_payload_and_shared_key() {
    let v = load().link_exchange;

    // The link parses back to the key and nonce it was built from.
    let parsed = parse_link_url(&v.url).expect("link parses");
    assert_eq!(hex::encode(parsed.initiator_public_key), v.initiator_public);
    assert_eq!(hex::encode(parsed.nonce), v.nonce);

    // The nonce-derived slots are SHA-256(nonce || label); the shipped
    // generator uses the same formula for a nonce of its own choosing.
    let nonce = hex32(&v.nonce);
    assert_eq!(v.handshake_gate, sha256_hex(&[&nonce, b"handshake"]));
    assert_eq!(
        v.presence_slot,
        sha256_hex(&[&nonce, b"initiator_presence"])
    );
    assert_eq!(v.epk_slot, sha256_hex(&[&nonce, b"epk"]));
    let (fresh, _) = initiator_generate_named("Vector");
    assert_eq!(
        fresh.handshake_slot,
        sha256_hex(&[&fresh.nonce, b"handshake"])
    );
    assert_eq!(
        fresh.presence_slot,
        sha256_hex(&[&fresh.nonce, b"initiator_presence"])
    );

    // Escrow: both roles derive the same gate; slots swap with the role;
    // the card key is HKDF(DH, card_key_info) and opens what the keys encrypt.
    let responder = X3DHKeyPair::from_bytes(hex32(&v.responder_secret));
    let dh = responder
        .diffie_hellman(&hex32(&v.initiator_public))
        .expect("dh");
    assert_eq!(hex::encode(*dh), v.dh);
    let as_initiator = EscrowKeys::derive(&dh[..], EscrowRole::Initiator);
    let as_responder = EscrowKeys::derive(&dh[..], EscrowRole::Responder);
    assert_eq!(as_initiator.gate_hash, v.escrow_gate);
    assert_eq!(as_responder.gate_hash, v.escrow_gate);
    assert_eq!(as_initiator.our_slot, v.escrow_slot_initiator);
    assert_eq!(as_initiator.their_slot, v.escrow_slot_responder);
    assert_eq!(as_responder.our_slot, v.escrow_slot_responder);
    let gate_by_hand = hex::encode(Sha256::digest(
        HKDF::derive_key(None, &dh[..], v.gate_info.as_bytes()).as_ref(),
    ));
    assert_eq!(gate_by_hand, v.escrow_gate);
    let sealed = as_initiator.encrypt_card(b"vector card").expect("encrypt");
    let card_key = SymmetricKey::from_bytes(hex32(&v.card_key));
    assert_eq!(
        decrypt(&card_key, &sealed).expect("decrypt"),
        b"vector card"
    );
    assert_eq!(
        hex::encode(*HKDF::derive_key(None, &dh[..], v.card_key_info.as_bytes())),
        v.card_key
    );

    // The payload is reproducible and parses with its signature verified
    // for this link and its two escrow keys.
    let identity = vector_identity(0x73, "Vector Link");
    assert_eq!(
        hex::encode(identity.signing_public_key()),
        v.initiator_identity_public
    );
    let x3dh_i = X3DHKeyPair::from_bytes(hex32(&v.initiator_x3dh_secret));
    let binding = LinkExchangeBinding {
        nonce,
        initiator_epk: hex32(&v.initiator_public),
        responder_epk: hex32(&v.responder_public),
    };
    let payload = serialize_link_card_payload(
        identity.signing_keypair(),
        x3dh_i.public_key(),
        &v.relay_url,
        &vector_card(),
        &binding,
    );
    assert_eq!(hex::encode(&payload), v.initiator_payload_v3);
    assert_eq!(payload[0], 0x03);
    let parsed_payload = parse_link_card_payload(&payload, &binding).expect("payload parses");
    assert_eq!(
        hex::encode(parsed_payload.identity_pubkey),
        v.initiator_identity_public
    );
    assert_eq!(
        hex::encode(parsed_payload.x3dh_pubkey),
        v.initiator_x3dh_public
    );
    assert_eq!(parsed_payload.relay_url, v.relay_url);
    assert_eq!(parsed_payload.card.id(), v.card_id);
    // The signature is Ed25519 over domain || nonce || initiator epk ||
    // responder epk || the JSON body that follows it in the payload.
    let mut message = v.bootstrap_domain.as_bytes().to_vec();
    message.extend_from_slice(&nonce);
    message.extend_from_slice(&binding.initiator_epk);
    message.extend_from_slice(&binding.responder_epk);
    message.extend_from_slice(&payload[65..]);
    assert_eq!(hex::encode(&payload[1..65]), v.bootstrap_signature);
    assert_eq!(
        hex::encode(identity.signing_keypair().sign(&message).as_bytes()),
        v.bootstrap_signature
    );
    let mut tampered = payload.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(parse_link_card_payload(&tampered, &binding).is_err());

    // The shared key is symmetric and HKDF(X25519(x3dh_i, x3dh_r), info).
    let x3dh_r = X3DHKeyPair::from_bytes(hex32(&v.responder_x3dh_secret));
    assert_eq!(
        hex::encode(
            derive_link_shared_key(&x3dh_i, x3dh_r.public_key())
                .expect("i")
                .as_bytes()
        ),
        v.link_shared_key
    );
    assert_eq!(
        hex::encode(
            derive_link_shared_key(&x3dh_r, x3dh_i.public_key())
                .expect("r")
                .as_bytes()
        ),
        v.link_shared_key
    );
    let raw = x3dh_i.diffie_hellman(x3dh_r.public_key()).expect("raw");
    assert_eq!(
        hex::encode(*HKDF::derive_key(
            None,
            &raw[..],
            v.link_shared_key_info.as_bytes()
        )),
        v.link_shared_key
    );
}

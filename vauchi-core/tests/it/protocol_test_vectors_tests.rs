// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Vauchi-specific known-answer vectors (vauchi/private#119).
//!
//! `tests/fixtures/test-vectors/vauchi-protocol-vectors-v1.json` holds
//! values the shipped functions produced once. Every run recomputes the
//! deterministic ones (key derivations, X3DH, chain steps, mailbox tokens)
//! and decrypts the recorded ciphertexts (a ratchet message, a v3 backup,
//! a v6 backup), so an independent implementation can check itself against
//! the file. The two exchange transcripts live in
//! `protocol_exchange_vectors_tests.rs` and share this fixture.
//!
//! Regenerate with `VAUCHI_WRITE_TEST_VECTORS=1`, without the `test-kdf`
//! feature: the backup vector must carry production Argon2id parameters.
//! The verify tests run in that mode too, against whichever copy of the
//! file they open; after a derivation change, run once more to see green.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use vauchi_core::Vauchi;
use vauchi_core::backup::{FullBackupIdentityData, export_full_backup, import_full_backup};
use vauchi_core::clock::FakeClock;
use vauchi_core::crypto::ratchet::SerializedRatchetState;
use vauchi_core::crypto::{
    ChainKey, DoubleRatchetState, HKDF, RatchetMessage, SigningKeyPair, SymmetricKey, X3DHKeyPair,
    decrypt, decrypt_with_ad,
};
use vauchi_core::exchange::X3DH;
use vauchi_core::identifiers::IdentityKey;
use vauchi_core::identity::device::DeviceInfo;
use vauchi_core::network::mailbox_token::{
    compute_device_mailbox_token, compute_device_sync_token, compute_mailbox_token,
};
use vauchi_core::recovery::{
    RecoveryProof, RecoveryVoucher, SEALED_RECOVERY_PROOF_INFO, SEALED_RECOVERY_PROOF_VERSION,
};

use super::protocol_exchange_vectors_tests::{
    LinkExchangeVector, QrSymmetricVector, compute_link_exchange, compute_qr_symmetric,
};

const FIXTURE: &str = "tests/fixtures/test-vectors/vauchi-protocol-vectors-v1.json";

/// Documented alongside the vectors so a reader can map each value to the
/// derivation that produced it.
const EXCHANGE_SEED_INFO: &str = "Vauchi_Exchange_Seed_v2";
const DEVICE_ID_INFO: &str = "Vauchi_Device_ID_v2";
const DEVICE_EXCHANGE_INFO: &str = "Vauchi_Device_Exchange_v2";
const X3DH_INFO: &str = "vauchi-x3dh-key-v2";
const ROOT_RATCHET_INFO: &str = "Vauchi_Root_Ratchet";
const CHAIN_KEY_INFO: &str = "Vauchi_Chain_Key";
const MESSAGE_KEY_INFO: &str = "Vauchi_Message_Key";
const MAILBOX_INFO: &str = "Vauchi_Mailbox_v1";
const MAILBOX_DEVICE_INFO: &str = "Vauchi_MailboxDevice_v1";
const DEVICE_SYNC_INFO: &str = "Vauchi_DeviceSyncRecipient_v1";
const BACKUP_INFO: &str = "vauchi-backup-v3";
const BACKUP_V6_INFO: &str = "vauchi-backup-v6-state";

#[derive(Serialize, Deserialize)]
pub(crate) struct Vectors {
    /// Why the file exists and how to read it; kept in the JSON so the
    /// published copy is self-describing.
    description: Vec<String>,
    identity: IdentityVector,
    x3dh: X3dhVector,
    ratchet: RatchetVector,
    mailbox_tokens: MailboxVector,
    backup: BackupVector,
    backup_v6: BackupV6Vector,
    pub(crate) qr_symmetric: QrSymmetricVector,
    pub(crate) link_exchange: LinkExchangeVector,
    recovery_proof: RecoveryProofVector,
}

/// The recovery proof as the relay stores it (Social Recovery B3): a plain
/// header of version byte and new key, then the postcard proof encrypted
/// under `HKDF(old_pk, info)` with the header as associated data.
#[derive(Serialize, Deserialize)]
struct RecoveryProofVector {
    info: String,
    old_master_seed: String,
    old_pk: String,
    new_master_seed: String,
    new_pk: String,
    helper_master_seed: String,
    helper_pk: String,
    threshold: u32,
    created_at: u64,
    voucher_timestamp: u64,
    sealing_key: String,
    plain_proof: String,
    sealed_proof: String,
}

#[derive(Serialize, Deserialize)]
struct IdentityVector {
    master_seed: String,
    signing_public_key: String,
    fingerprint: String,
    exchange_seed_info: String,
    exchange_seed: String,
    exchange_public_key: String,
    device_index: u32,
    device_id_info: String,
    device_id: String,
    device_exchange_info: String,
    device_exchange_public_key: String,
}

#[derive(Serialize, Deserialize)]
struct X3dhVector {
    info: String,
    initiator_static_secret: String,
    initiator_static_public: String,
    responder_static_secret: String,
    responder_static_public: String,
    initiator_ephemeral_secret: String,
    initiator_ephemeral_public: String,
    dh1: String,
    dh2: String,
    shared_key: String,
}

#[derive(Serialize, Deserialize)]
struct RatchetVector {
    root_info: String,
    chain_key_info: String,
    message_key_info: String,
    x3dh_secret: String,
    responder_dh_secret: String,
    responder_dh_public: String,
    initiator_dh_secret: String,
    initiator_dh_public: String,
    dh_output: String,
    root_key: String,
    send_chain_key_0: String,
    message_key_0: String,
    send_chain_key_1: String,
    plaintext: String,
    message: RatchetMessageVector,
}

#[derive(Serialize, Deserialize)]
struct RatchetMessageVector {
    dh_public: String,
    dh_generation: u32,
    message_index: u32,
    previous_chain_length: u32,
    associated_data: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct MailboxVector {
    contact_info: String,
    device_contact_info: String,
    device_sync_info: String,
    shared_key: String,
    recipient_identity_key: String,
    recipient_device_id: String,
    master_seed: String,
    day_epoch: u64,
    contact_token: String,
    device_contact_token: String,
    device_sync_token: String,
}

#[derive(Serialize, Deserialize)]
struct BackupVector {
    password: String,
    argon2id: Argon2Params,
    hkdf_info: String,
    display_name: String,
    master_seed: String,
    device_index: u32,
    device_name: String,
    created_at: u64,
    salt: String,
    argon2id_base_key: String,
    key: String,
    backup: String,
}

/// The password backup the app writes today: the clear 17-byte header is
/// the AEAD's associated data, so a changed version byte or salt fails the
/// tag instead of deriving a wrong key.
#[derive(Serialize, Deserialize)]
struct BackupV6Vector {
    password: String,
    argon2id: Argon2Params,
    hkdf_info: String,
    display_name: String,
    master_seed: String,
    device_index: u32,
    device_name: String,
    created_at: u64,
    salt: String,
    argon2id_base_key: String,
    key: String,
    header: String,
    backup: String,
}

#[derive(Serialize, Deserialize)]
struct Argon2Params {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    output_bytes: u32,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE)
}

fn writing() -> bool {
    std::env::var_os("VAUCHI_WRITE_TEST_VECTORS").is_some()
}

pub(crate) fn seed(byte: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = byte.wrapping_add(i as u8);
    }
    out
}

pub(crate) fn hex32(s: &str) -> [u8; 32] {
    hex::decode(s).expect("hex").try_into().expect("32 bytes")
}

pub(crate) fn load() -> Vectors {
    let text = std::fs::read_to_string(fixture_path()).expect("vector fixture present");
    serde_json::from_str(&text).expect("vector fixture parses")
}

// ── identity ──────────────────────────────────────────────────────────

fn compute_identity(master_seed: [u8; 32]) -> IdentityVector {
    let signing = SigningKeyPair::from_seed(&master_seed);
    let exchange_seed = HKDF::derive_key(None, &master_seed, EXCHANGE_SEED_INFO.as_bytes());
    let exchange = X3DHKeyPair::from_bytes(*exchange_seed);
    let device = DeviceInfo::derive(&master_seed, 0, "vector-device".to_string(), 0);
    IdentityVector {
        master_seed: hex::encode(master_seed),
        signing_public_key: hex::encode(signing.public_key().as_bytes()),
        fingerprint: signing.public_key().fingerprint(),
        exchange_seed_info: EXCHANGE_SEED_INFO.to_string(),
        exchange_seed: hex::encode(*exchange_seed),
        exchange_public_key: hex::encode(exchange.public_key()),
        device_index: 0,
        device_id_info: DEVICE_ID_INFO.to_string(),
        device_id: hex::encode(device.device_id()),
        device_exchange_info: DEVICE_EXCHANGE_INFO.to_string(),
        device_exchange_public_key: hex::encode(device.exchange_public_key()),
    }
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn identity_vector_matches_the_shipped_key_derivations() {
    let v = load().identity;
    let got = compute_identity(hex32(&v.master_seed));
    assert_eq!(got.signing_public_key, v.signing_public_key);
    assert_eq!(got.fingerprint, v.fingerprint);
    assert_eq!(got.exchange_seed, v.exchange_seed);
    assert_eq!(got.exchange_public_key, v.exchange_public_key);
    assert_eq!(got.device_id, v.device_id);
    assert_eq!(got.device_exchange_public_key, v.device_exchange_public_key);
    // The device derivations fold the little-endian index into the info.
    let mut info = DEVICE_ID_INFO.as_bytes().to_vec();
    info.extend_from_slice(&v.device_index.to_le_bytes());
    let by_hand = HKDF::derive_key(None, &hex32(&v.master_seed), &info);
    assert_eq!(hex::encode(*by_hand), v.device_id);
}

// ── X3DH ──────────────────────────────────────────────────────────────

fn compute_x3dh() -> (X3dhVector, SymmetricKey) {
    let initiator = X3DHKeyPair::from_bytes(seed(0x10));
    let responder = X3DHKeyPair::from_bytes(seed(0x20));
    let ephemeral = X3DHKeyPair::from_bytes(seed(0x30));
    let dh1 = initiator
        .diffie_hellman(responder.public_key())
        .expect("dh1");
    let dh2 = ephemeral
        .diffie_hellman(responder.public_key())
        .expect("dh2");
    let shared =
        X3DH::respond(&responder, initiator.public_key(), ephemeral.public_key()).expect("respond");
    let vector = X3dhVector {
        info: X3DH_INFO.to_string(),
        initiator_static_secret: hex::encode(*initiator.secret_bytes()),
        initiator_static_public: hex::encode(initiator.public_key()),
        responder_static_secret: hex::encode(*responder.secret_bytes()),
        responder_static_public: hex::encode(responder.public_key()),
        initiator_ephemeral_secret: hex::encode(*ephemeral.secret_bytes()),
        initiator_ephemeral_public: hex::encode(ephemeral.public_key()),
        dh1: hex::encode(*dh1),
        dh2: hex::encode(*dh2),
        shared_key: hex::encode(shared.as_bytes()),
    };
    (vector, shared)
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn x3dh_vector_matches_the_responder_derivation_and_its_formula() {
    let v = load().x3dh;
    let responder = X3DHKeyPair::from_bytes(hex32(&v.responder_static_secret));
    let shared = X3DH::respond(
        &responder,
        &hex32(&v.initiator_static_public),
        &hex32(&v.initiator_ephemeral_public),
    )
    .expect("respond");
    assert_eq!(hex::encode(shared.as_bytes()), v.shared_key);

    // HKDF-SHA256(salt = none, ikm = DH1 || DH2, info) is the whole derivation.
    let mut ikm = hex::decode(&v.dh1).expect("dh1");
    ikm.extend(hex::decode(&v.dh2).expect("dh2"));
    let by_hand = HKDF::derive_key(None, &ikm, v.info.as_bytes());
    assert_eq!(hex::encode(*by_hand), v.shared_key);
}

// ── Double Ratchet ────────────────────────────────────────────────────

fn compute_ratchet(x3dh_secret: &SymmetricKey) -> RatchetVector {
    let responder_dh = X3DHKeyPair::from_bytes(seed(0x40));
    let initiator_dh = X3DHKeyPair::from_bytes(seed(0x50));
    let dh_output = initiator_dh
        .diffie_hellman(responder_dh.public_key())
        .expect("dh");
    let (root_key, send_chain_key) = HKDF::derive_key_pair(
        Some(x3dh_secret.as_bytes()),
        &*dh_output,
        ROOT_RATCHET_INFO.as_bytes(),
    );
    let (message_key_0, _) = ChainKey::new(*send_chain_key).ratchet().expect("step");

    // The initiator's state as `initialize_initiator` builds it, with the
    // DH key pinned so the vector is reproducible.
    let mut initiator = DoubleRatchetState::deserialize(SerializedRatchetState {
        version: vauchi_core::crypto::RATCHET_STATE_VERSION,
        root_key: *root_key,
        our_dh_secret: *initiator_dh.secret_bytes(),
        their_dh: Some(*responder_dh.public_key()),
        send_chain: Some((*send_chain_key, 0)),
        recv_chain: None,
        dh_generation: 0,
        send_message_count: 0,
        recv_message_count: 0,
        previous_send_chain_length: 0,
        skipped_keys: Vec::new(),
    })
    .expect("state");
    let plaintext = b"vauchi test vector";
    let message = initiator.encrypt(plaintext).expect("encrypt");
    let send_chain_key_1 = send_chain_after(&initiator, 1);
    let mut responder =
        DoubleRatchetState::initialize_responder(x3dh_secret, X3DHKeyPair::from_bytes(seed(0x40)));
    assert_eq!(
        responder.decrypt(&message).expect("decrypt"),
        plaintext.to_vec()
    );

    RatchetVector {
        root_info: ROOT_RATCHET_INFO.to_string(),
        chain_key_info: CHAIN_KEY_INFO.to_string(),
        message_key_info: MESSAGE_KEY_INFO.to_string(),
        x3dh_secret: hex::encode(x3dh_secret.as_bytes()),
        responder_dh_secret: hex::encode(*responder_dh.secret_bytes()),
        responder_dh_public: hex::encode(responder_dh.public_key()),
        initiator_dh_secret: hex::encode(*initiator_dh.secret_bytes()),
        initiator_dh_public: hex::encode(initiator_dh.public_key()),
        dh_output: hex::encode(*dh_output),
        root_key: hex::encode(*root_key),
        send_chain_key_0: hex::encode(*send_chain_key),
        message_key_0: hex::encode(message_key_0.symmetric_key().as_bytes()),
        send_chain_key_1: hex::encode(send_chain_key_1),
        plaintext: hex::encode(plaintext),
        message: RatchetMessageVector {
            dh_public: hex::encode(message.dh_public),
            dh_generation: message.dh_generation,
            message_index: message.message_index,
            previous_chain_length: message.previous_chain_length,
            associated_data: hex::encode(associated_data(&message)),
            ciphertext: hex::encode(&message.ciphertext),
        },
    }
}

/// `ChainKey` keeps its bytes private; the serialized state is the public
/// path that shows the chain key a send left behind.
fn send_chain_after(state: &DoubleRatchetState, generation: u32) -> [u8; 32] {
    let (key, r#gen) = state.serialize().send_chain.expect("send chain");
    assert_eq!(r#gen, generation);
    key
}

fn associated_data(m: &RatchetMessage) -> [u8; 44] {
    let mut ad = [0u8; 44];
    ad[0..32].copy_from_slice(&m.dh_public);
    ad[32..36].copy_from_slice(&m.dh_generation.to_be_bytes());
    ad[36..40].copy_from_slice(&m.message_index.to_be_bytes());
    ad[40..44].copy_from_slice(&m.previous_chain_length.to_be_bytes());
    ad
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn ratchet_vector_matches_the_root_and_chain_steps() {
    let v = load().ratchet;
    let x3dh_secret = hex32(&v.x3dh_secret);
    let (root_key, send_chain_key) = HKDF::derive_key_pair(
        Some(&x3dh_secret),
        &hex32(&v.dh_output),
        v.root_info.as_bytes(),
    );
    assert_eq!(hex::encode(*root_key), v.root_key);
    assert_eq!(hex::encode(*send_chain_key), v.send_chain_key_0);

    let (message_key, next) = ChainKey::new(*send_chain_key).ratchet().expect("step");
    assert_eq!(
        hex::encode(message_key.symmetric_key().as_bytes()),
        v.message_key_0
    );
    assert_eq!(next.generation(), 1);

    // Both chain outputs are plain HKDF expansions of the chain key.
    let mk = HKDF::derive_key(None, &*send_chain_key, v.message_key_info.as_bytes());
    let ck = HKDF::derive_key(None, &*send_chain_key, v.chain_key_info.as_bytes());
    assert_eq!(hex::encode(*mk), v.message_key_0);
    assert_eq!(hex::encode(*ck), v.send_chain_key_1);

    // The shipped send path leaves the same next chain key behind.
    let mut state = DoubleRatchetState::deserialize(SerializedRatchetState {
        version: vauchi_core::crypto::RATCHET_STATE_VERSION,
        root_key: *root_key,
        our_dh_secret: hex32(&v.initiator_dh_secret),
        their_dh: Some(hex32(&v.responder_dh_public)),
        send_chain: Some((*send_chain_key, 0)),
        recv_chain: None,
        dh_generation: 0,
        send_message_count: 0,
        recv_message_count: 0,
        previous_send_chain_length: 0,
        skipped_keys: Vec::new(),
    })
    .expect("state");
    state.encrypt(b"any").expect("encrypt");
    assert_eq!(hex::encode(send_chain_after(&state, 1)), v.send_chain_key_1);
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn ratchet_vector_message_decrypts_on_a_fresh_responder() {
    let v = load().ratchet;
    let message = RatchetMessage {
        dh_public: hex32(&v.message.dh_public),
        dh_generation: v.message.dh_generation,
        message_index: v.message.message_index,
        previous_chain_length: v.message.previous_chain_length,
        ciphertext: hex::decode(&v.message.ciphertext).expect("ciphertext"),
    };
    assert_eq!(
        hex::encode(associated_data(&message)),
        v.message.associated_data
    );
    assert_eq!(v.message.dh_public, v.initiator_dh_public);

    let x3dh_secret = SymmetricKey::from_bytes(hex32(&v.x3dh_secret));
    let responder_dh = X3DHKeyPair::from_bytes(hex32(&v.responder_dh_secret));
    let mut responder = DoubleRatchetState::initialize_responder(&x3dh_secret, responder_dh);
    let plaintext = responder.decrypt(&message).expect("decrypt");
    assert_eq!(hex::encode(plaintext), v.plaintext);
}

// ── mailbox tokens ────────────────────────────────────────────────────

fn compute_recovery_proof() -> RecoveryProofVector {
    let old_master_seed = seed(0xA0);
    let new_master_seed = seed(0xB0);
    let helper_master_seed = seed(0xC0);
    let old = SigningKeyPair::from_seed(&old_master_seed);
    let new = SigningKeyPair::from_seed(&new_master_seed);
    let helper = SigningKeyPair::from_seed(&helper_master_seed);
    let created_at = 1_700_000_000;
    let voucher_timestamp = created_at + 60;
    let mut proof = RecoveryProof::new(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        1,
        created_at,
    );
    proof
        .add_voucher(RecoveryVoucher::create(
            old.public_key().as_bytes(),
            new.public_key().as_bytes(),
            &helper,
            None,
            voucher_timestamp,
        ))
        .expect("one helper voucher");
    let sealing_key = HKDF::derive_key(
        None,
        old.public_key().as_bytes(),
        SEALED_RECOVERY_PROOF_INFO,
    );
    RecoveryProofVector {
        info: String::from_utf8(SEALED_RECOVERY_PROOF_INFO.to_vec()).unwrap(),
        old_master_seed: hex::encode(old_master_seed),
        old_pk: hex::encode(old.public_key().as_bytes()),
        new_master_seed: hex::encode(new_master_seed),
        new_pk: hex::encode(new.public_key().as_bytes()),
        helper_master_seed: hex::encode(helper_master_seed),
        helper_pk: hex::encode(helper.public_key().as_bytes()),
        threshold: 1,
        created_at,
        voucher_timestamp,
        sealing_key: hex::encode(*sealing_key),
        plain_proof: hex::encode(proof.to_bytes().unwrap()),
        sealed_proof: hex::encode(proof.seal_for_relay().unwrap()),
    }
}

fn compute_mailbox() -> MailboxVector {
    let shared_key = seed(0x60);
    let recipient_identity_key = seed(0x70);
    let recipient_device_id = seed(0x80);
    let master_seed = seed(0x90);
    let day_epoch = 20_750;
    MailboxVector {
        contact_info: MAILBOX_INFO.to_string(),
        device_contact_info: MAILBOX_DEVICE_INFO.to_string(),
        device_sync_info: DEVICE_SYNC_INFO.to_string(),
        shared_key: hex::encode(shared_key),
        recipient_identity_key: hex::encode(recipient_identity_key),
        recipient_device_id: hex::encode(recipient_device_id),
        master_seed: hex::encode(master_seed),
        day_epoch,
        contact_token: hex::encode(
            compute_mailbox_token(&shared_key, &recipient_identity_key, day_epoch).as_bytes(),
        ),
        device_contact_token: hex::encode(
            compute_device_mailbox_token(
                &shared_key,
                &recipient_identity_key,
                &recipient_device_id,
                day_epoch,
            )
            .as_bytes(),
        ),
        device_sync_token: hex::encode(
            compute_device_sync_token(&master_seed, &recipient_device_id, day_epoch).as_bytes(),
        ),
    }
}

/// The stored proof opens under the recorded old key and under no other,
/// and the sealing key and header follow the stated formulas (SC-17,
/// vauchi/private#936).
// @scenario: contact_recovery :: Relay stores recovery proof privately
#[test]
fn recovery_proof_vector_opens_under_its_old_key_and_not_another() {
    let v = load().recovery_proof;
    let old_pk = IdentityKey::from_bytes(hex32(&v.old_pk));
    let sealed = hex::decode(&v.sealed_proof).unwrap();

    assert_eq!(sealed[0], SEALED_RECOVERY_PROOF_VERSION);
    assert_eq!(hex::encode(&sealed[1..33]), v.new_pk);
    assert_eq!(v.info.as_bytes(), SEALED_RECOVERY_PROOF_INFO);
    assert_eq!(
        hex::encode(*HKDF::derive_key(
            None,
            old_pk.as_bytes(),
            v.info.as_bytes()
        )),
        v.sealing_key
    );

    let opened = RecoveryProof::open_sealed(&sealed, &old_pk).unwrap();
    assert_eq!(hex::encode(opened.old_pk().as_bytes()), v.old_pk);
    assert_eq!(hex::encode(opened.new_pk().as_bytes()), v.new_pk);
    assert_eq!(opened.threshold(), v.threshold);
    assert_eq!(opened.voucher_count(), 1);
    assert_eq!(
        hex::encode(opened.vouchers()[0].voucher_pk().as_bytes()),
        v.helper_pk
    );
    assert_eq!(hex::encode(opened.to_bytes().unwrap()), v.plain_proof);

    let by_hand = decrypt_with_ad(
        &SymmetricKey::from_bytes(hex32(&v.sealing_key)),
        &sealed[33..],
        &sealed[..33],
    )
    .unwrap();
    assert_eq!(hex::encode(by_hand), v.plain_proof);

    let other = IdentityKey::from_bytes(hex32(&v.new_pk));
    assert!(RecoveryProof::open_sealed(&sealed, &other).is_err());
    let helper = SigningKeyPair::from_seed(&hex32(&v.helper_master_seed));
    assert_eq!(hex::encode(helper.public_key().as_bytes()), v.helper_pk);
}

// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn mailbox_token_vectors_match_the_three_derivations() {
    let v = load().mailbox_tokens;
    let shared_key = hex32(&v.shared_key);
    let recipient = hex32(&v.recipient_identity_key);
    let device_id = hex32(&v.recipient_device_id);
    let master_seed = hex32(&v.master_seed);
    assert_eq!(
        hex::encode(compute_mailbox_token(&shared_key, &recipient, v.day_epoch).as_bytes()),
        v.contact_token
    );
    assert_eq!(
        hex::encode(
            compute_device_mailbox_token(&shared_key, &recipient, &device_id, v.day_epoch)
                .as_bytes()
        ),
        v.device_contact_token
    );
    assert_eq!(
        hex::encode(compute_device_sync_token(&master_seed, &device_id, v.day_epoch).as_bytes()),
        v.device_sync_token
    );

    // info = domain || recipient key [|| device id] || day (big-endian u64).
    let mut info = v.contact_info.as_bytes().to_vec();
    info.extend_from_slice(&recipient);
    info.extend_from_slice(&v.day_epoch.to_be_bytes());
    let by_hand = HKDF::derive_key(None, &shared_key, &info);
    assert_eq!(hex::encode(*by_hand), v.contact_token);
}

// ── backup ────────────────────────────────────────────────────────────

fn compute_backup() -> BackupVector {
    let master_seed = seed(0xa0);
    let password = "correct horse battery staple";
    let identity = FullBackupIdentityData {
        display_name: "Vector".to_string(),
        master_seed,
        device_index: 0,
        device_name: "vector-device".to_string(),
    };
    let created_at = 1_793_000_000;
    let backup =
        export_full_backup(&identity, &[], None, &[], password, created_at).expect("export");
    let salt: [u8; 16] = backup[1..17].try_into().expect("salt");
    let base_key =
        vauchi_core::crypto::derive_key_argon2id(password.as_bytes(), &salt).expect("argon2id");
    let prk = HKDF::extract(None, base_key.as_bytes());
    let key = HKDF::expand(&prk, BACKUP_INFO.as_bytes(), 32).expect("expand");
    BackupVector {
        password: password.to_string(),
        argon2id: Argon2Params {
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 4,
            output_bytes: 32,
        },
        hkdf_info: BACKUP_INFO.to_string(),
        display_name: identity.display_name.clone(),
        master_seed: hex::encode(master_seed),
        device_index: identity.device_index,
        device_name: identity.device_name.clone(),
        created_at,
        salt: hex::encode(salt),
        argon2id_base_key: hex::encode(base_key.as_bytes()),
        key: hex::encode(&key),
        backup: hex::encode(&backup),
    }
}

/// Opens the recorded backup from the recorded Argon2id output, which
/// holds whether or not `test-kdf` weakened the parameters of this build.
// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn backup_vector_decrypts_from_the_recorded_argon2id_output() {
    let v = load().backup;
    let backup = hex::decode(&v.backup).expect("backup");
    assert_eq!(backup[0], 0x03, "v3 version byte");
    assert_eq!(hex::encode(&backup[1..17]), v.salt);

    let prk = HKDF::extract(None, &hex32(&v.argon2id_base_key));
    let key = HKDF::expand(&prk, v.hkdf_info.as_bytes(), 32).expect("expand");
    assert_eq!(hex::encode(&key), v.key);

    let key = SymmetricKey::from_bytes(key.try_into().expect("32 bytes"));
    let compressed = decrypt(&key, &backup[17..]).expect("decrypt");
    assert_eq!(compressed[0], 0x78, "zlib stream");
    let mut json = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::ZlibDecoder::new(compressed.as_slice()),
        &mut json,
    )
    .expect("inflate");
    let envelope: serde_json::Value = serde_json::from_slice(&json).expect("json");
    assert_eq!(envelope["version"], 3);
    assert_eq!(envelope["created_at"], v.created_at);
    assert_eq!(
        envelope["sections"]["identity"]["display_name"],
        v.display_name
    );
    assert_eq!(
        envelope["sections"]["identity"]["device_name"],
        v.device_name
    );
}

/// The whole import, password in: only a build with production Argon2id
/// parameters can reproduce the recorded base key.
// @scenario: security :: Correct algorithms used
// @internal
#[cfg(not(feature = "test-kdf"))]
#[test]
fn backup_vector_imports_with_production_argon2id() {
    let v = load().backup;
    let backup = hex::decode(&v.backup).expect("backup");
    let salt: [u8; 16] = backup[1..17].try_into().expect("salt");
    let base_key =
        vauchi_core::crypto::derive_key_argon2id(v.password.as_bytes(), &salt).expect("argon2id");
    assert_eq!(hex::encode(base_key.as_bytes()), v.argon2id_base_key);

    let envelope = import_full_backup(&backup, &v.password).expect("import");
    assert_eq!(envelope.version, 3);
    assert_eq!(envelope.created_at, v.created_at);
    assert_eq!(envelope.sections.identity.display_name, v.display_name);
    assert_eq!(envelope.sections.identity.device_index, v.device_index);
    let seed =
        vauchi_core::backup::extract_master_seed(&envelope.sections.identity).expect("master seed");
    assert_eq!(hex::encode(*seed), v.master_seed);
}

// ── backup v6 ─────────────────────────────────────────────────────────

const V6_SEED_BYTE: u8 = 0xb0;
const V6_PASSWORD: &str = "correct horse battery staple";
const V6_CREATED_AT: u64 = 1_793_100_000;

/// A `Vauchi` whose identity has a known seed and whose clock stands at
/// `V6_CREATED_AT`: the only public way to a fixed seed is to restore one,
/// so the v3 exporter seeds the v6 vector's identity.
fn vauchi_with_vector_identity() -> Vauchi {
    let identity = FullBackupIdentityData {
        display_name: "Vector".to_string(),
        master_seed: seed(V6_SEED_BYTE),
        device_index: 0,
        device_name: "vector-device".to_string(),
    };
    let legacy = export_full_backup(&identity, &[], None, &[], V6_PASSWORD, V6_CREATED_AT)
        .expect("v3 export");
    let clock = FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(V6_CREATED_AT));
    let mut vauchi = Vauchi::in_memory_with_clock(clock.shared()).expect("in-memory vauchi");
    vauchi
        .import_full_backup(&hex::encode(legacy), V6_PASSWORD)
        .expect("v3 import");
    vauchi
}

fn compute_backup_v6() -> BackupV6Vector {
    let vauchi = vauchi_with_vector_identity();
    let backup =
        hex::decode(vauchi.export_full_backup(V6_PASSWORD).expect("v6 export")).expect("hex");
    let salt: [u8; 16] = backup[1..17].try_into().expect("salt");
    let base_key =
        vauchi_core::crypto::derive_key_argon2id(V6_PASSWORD.as_bytes(), &salt).expect("argon2id");
    let prk = HKDF::extract(None, base_key.as_bytes());
    let key = HKDF::expand(&prk, BACKUP_V6_INFO.as_bytes(), 32).expect("expand");
    BackupV6Vector {
        password: V6_PASSWORD.to_string(),
        argon2id: Argon2Params {
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 4,
            output_bytes: 32,
        },
        hkdf_info: BACKUP_V6_INFO.to_string(),
        display_name: "Vector".to_string(),
        master_seed: hex::encode(seed(V6_SEED_BYTE)),
        device_index: 0,
        device_name: "vector-device".to_string(),
        created_at: V6_CREATED_AT,
        salt: hex::encode(salt),
        argon2id_base_key: hex::encode(base_key.as_bytes()),
        key: hex::encode(&key),
        header: hex::encode(&backup[..17]),
        backup: hex::encode(&backup),
    }
}

// @claim: S13
/// Opens the recorded v6 backup from the recorded Argon2id output with the
/// header as associated data, and shows the header is bound: the same key
/// with one salt byte changed opens nothing.
// @scenario: security :: Correct algorithms used
// @internal
#[test]
fn backup_v6_vector_decrypts_under_its_header_and_not_under_a_changed_one() {
    let v = load().backup_v6;
    let backup = hex::decode(&v.backup).expect("backup");
    let header = hex::decode(&v.header).expect("header");
    assert_eq!(backup[0], 0x06, "v6 version byte");
    assert_eq!(header.len(), 17);
    assert_eq!(&backup[..17], header.as_slice());
    assert_eq!(hex::encode(&backup[1..17]), v.salt);
    assert_eq!(v.hkdf_info, BACKUP_V6_INFO);

    let prk = HKDF::extract(None, &hex32(&v.argon2id_base_key));
    let key = HKDF::expand(&prk, v.hkdf_info.as_bytes(), 32).expect("expand");
    assert_eq!(hex::encode(&key), v.key);
    let key = SymmetricKey::from_bytes(key.try_into().expect("32 bytes"));

    let mut other_header = header.clone();
    other_header[1] ^= 0x01;
    assert!(
        decrypt_with_ad(&key, &backup[17..], &other_header).is_err(),
        "a changed salt in the associated data must fail the tag"
    );

    let compressed = decrypt_with_ad(&key, &backup[17..], &header).expect("decrypt");
    assert_eq!(compressed[0], 0x78, "zlib stream");
    let mut json = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::ZlibDecoder::new(compressed.as_slice()),
        &mut json,
    )
    .expect("inflate");
    let snapshot: serde_json::Value = serde_json::from_slice(&json).expect("json");
    assert_eq!(snapshot["version"], 6);
    assert_eq!(snapshot["created_at"], v.created_at);
    assert_eq!(snapshot["identity"]["display_name"], v.display_name);
    assert_eq!(snapshot["identity"]["device_name"], v.device_name);
    assert_eq!(snapshot["identity"]["device_index"], v.device_index);
    let seed_b64 = snapshot["identity"]["master_seed_b64"]
        .as_str()
        .expect("master_seed_b64");
    let seed_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, seed_b64)
        .expect("base64 seed");
    assert_eq!(hex::encode(seed_bytes), v.master_seed);
}

// @claim: S13
/// The whole v6 import, password in, on a fresh installation; a wrong
/// password restores nothing.
// @scenario: security :: Correct algorithms used
// @internal
#[cfg(not(feature = "test-kdf"))]
#[test]
fn backup_v6_vector_imports_with_production_argon2id_and_refuses_a_wrong_password() {
    let v = load().backup_v6;
    let salt = hex::decode(&v.salt).expect("salt");
    let base_key = vauchi_core::crypto::derive_key_argon2id(
        v.password.as_bytes(),
        salt.as_slice().try_into().expect("16 bytes"),
    )
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
        .expect("v6 import");
    let identity = vauchi.identity().expect("restored identity");
    assert_eq!(hex::encode(identity.master_seed()), v.master_seed);
    assert_eq!(identity.display_name(), v.display_name);
    assert_eq!(identity.device_index(), v.device_index);
}

// ── regeneration ──────────────────────────────────────────────────────

/// `VAUCHI_WRITE_TEST_VECTORS=1 cargo test -p vauchi-core --features testing
/// --test it protocol_test_vectors` rewrites the fixture.
// @scenario: security :: Correct algorithms used
// @internal
#[cfg(not(feature = "test-kdf"))]
#[test]
fn write_test_vectors_when_asked() {
    if !writing() {
        return;
    }
    let (x3dh, shared) = compute_x3dh();
    let vectors = Vectors {
        description: vec![
            "Vauchi known-answer vectors, produced by vauchi-core's shipped functions.".into(),
            "All byte strings are lower-case hex. KDF: HKDF-SHA256 (RFC 5869); AEAD: \
             XChaCha20-Poly1305 in the 0x02/0x03 tagged format; curves: X25519 and \
             Ed25519."
                .into(),
            "identity: Ed25519 key from the master seed; X25519 exchange key from \
             HKDF(seed, info); per-device id and exchange key from HKDF(seed, info || \
             index as little-endian u32)."
                .into(),
            "x3dh: shared_key = HKDF(salt = none, ikm = DH1 || DH2, info) with DH1 = \
             X25519(initiator static, responder static) and DH2 = X25519(initiator \
             ephemeral, responder static), as X3DH::respond computes it."
                .into(),
            "ratchet: (root_key || send_chain_key_0) = HKDF(salt = x3dh_secret, ikm = \
             DH(initiator dh, responder dh), root_info, 64 bytes); message_key_0 = \
             HKDF(chain key, message_key_info); send_chain_key_1 = HKDF(chain key, \
             chain_key_info). message.ciphertext encrypts pad(plaintext) with \
             message_key_0 and the 44-byte header as associated data; the nonce and \
             the padding bytes are random, so only decryption is reproducible."
                .into(),
            "mailbox_tokens: token = HKDF(ikm, info || recipient key [|| device id] || \
             day_epoch as big-endian u64) with ikm the shared key (contact tokens) or \
             the master seed (device-sync token)."
                .into(),
            "backup: 0x03 || salt(16) || XChaCha20-Poly1305(key, zlib(json)); key = \
             HKDF-expand(HKDF-extract(none, Argon2id(password, salt)), hkdf_info, 32). \
             This is the legacy format the importer still accepts."
                .into(),
            "backup_v6: header = 0x06 || salt(16); file = header || \
             XChaCha20-Poly1305(key, zlib(json), associated data = header) in the \
             0x03 tagged format; key = HKDF-expand(HKDF-extract(none, \
             Argon2id(password, salt)), hkdf_info, 32). This is the format the app \
             exports today; the nonce is random, so only decryption is reproducible."
                .into(),
            "qr_symmetric: the mutual QR exchange. shared_key = HKDF(salt = none, ikm = \
             X25519(eph_a, eph_b), info_prefix || min(id_a, id_b) || max(id_a, id_b) || \
             min(eph_a, eph_b) || max(eph_a, eph_b)); each side's confirmation token = \
             HKDF(DH, confirmation_info || its own identity key); the confirmation \
             escrow gate and slots are hex(SHA-256(HKDF(DH, info))). The side with the \
             smaller identity key is the initiator and owns the init slot."
                .into(),
            "link_exchange: the link carries the initiator's ephemeral X25519 key and a \
             32-byte nonce, base64url without padding; handshake gate, presence slot \
             and epk slot are hex(SHA-256(nonce || label)); escrow gate and slots are \
             hex(SHA-256(HKDF(DH, info))) and the card key HKDF(DH, card_key_info) with \
             DH = X25519(responder ephemeral, initiator ephemeral); the v3 payload is \
             0x03 || signature(64) || JSON body, the Ed25519 signature over \
             bootstrap_domain || nonce || initiator ephemeral || responder ephemeral \
             || JSON body; link_shared_key = HKDF(X25519(x3dh_i, x3dh_r), \
             link_shared_key_info)."
                .into(),
        ],
        identity: compute_identity(seed(0x01)),
        ratchet: compute_ratchet(&shared),
        x3dh,
        mailbox_tokens: compute_mailbox(),
        backup: compute_backup(),
        backup_v6: compute_backup_v6(),
        qr_symmetric: compute_qr_symmetric(),
        link_exchange: compute_link_exchange(),
        recovery_proof: compute_recovery_proof(),
    };
    let mut text = serde_json::to_string_pretty(&vectors).expect("serialize");
    text.push('\n');
    // Written beside and renamed over, so a verify test running at the
    // same time never opens a half-written file.
    let staged = fixture_path().with_extension("json.tmp");
    std::fs::write(&staged, text).expect("write fixture");
    std::fs::rename(&staged, fixture_path()).expect("rename fixture");
}

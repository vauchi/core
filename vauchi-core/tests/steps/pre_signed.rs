// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use cucumber::{given, then, when};
use vauchi_core::api::PreSignedShredMessages;
use vauchi_core::crypto::signing::{Signature, verify_signature};

use crate::VauchiWorld;

// ── Pre-signed shred messages ─────────────────────────────────────────────

/// Generates pre-signed shred messages and stores the purge request fields
/// as JSON in world.pending_value for subsequent verification steps.
#[given("I have a pre-signed purge request")]
fn have_pre_signed_purge_request(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().expect("identity should exist");
    let msgs = PreSignedShredMessages::generate(identity, 1_700_000_000);
    let req = &msgs.purge_request;
    let json = serde_json::json!({
        "pk":    req.public_key.to_vec(),
        "sig":   req.signature,
        "token": req.purge_token.to_vec(),
        "ts":    req.timestamp,
    });
    world.pending_value = Some(json.to_string());
}

fn parse_purge_request(world: &VauchiWorld) -> ([u8; 32], [u8; 64], [u8; 32], u64) {
    let json_str = world
        .pending_value
        .as_deref()
        .expect("no purge request stored in world.pending_value");
    let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let to_vec = |key: &str| -> Vec<u8> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect()
    };
    let pk: [u8; 32] = to_vec("pk").try_into().unwrap();
    let sig: [u8; 64] = to_vec("sig").try_into().unwrap();
    let token: [u8; 32] = to_vec("token").try_into().unwrap();
    let ts = v["ts"].as_u64().unwrap();
    (pk, sig, token, ts)
}

fn purge_message(pk: &[u8; 32], token: &[u8; 32], ts: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(32 + 32 + 8);
    msg.extend_from_slice(pk);
    msg.extend_from_slice(token);
    msg.extend_from_slice(&ts.to_be_bytes());
    msg
}

/// Verifies the Ed25519 signature over (public_key || purge_token || timestamp).
#[then("the signature should be valid over (public_key || purge_token || timestamp)")]
fn signature_valid(world: &mut VauchiWorld) {
    let (pk, sig_bytes, token, ts) = parse_purge_request(world);
    let message = purge_message(&pk, &token, ts);
    let sig = Signature::from_bytes(sig_bytes);
    assert!(
        verify_signature(&pk, &message, &sig),
        "purge request signature should be valid over (pk || token || ts)"
    );
}

/// No-op: relay acceptance is a network-level concern verified by relay tests.
#[then("the relay should accept the signature using my public key")]
fn relay_accepts_signature(_world: &mut VauchiWorld) {}

/// Flips a bit in the stored purge token to simulate tampering.
#[when("the purge token is modified")]
fn modify_purge_token(world: &mut VauchiWorld) {
    let (pk, sig, mut token, ts) = parse_purge_request(world);
    token[0] ^= 0xFF;
    let json = serde_json::json!({
        "pk":    pk.to_vec(),
        "sig":   sig.to_vec(),
        "token": token.to_vec(),
        "ts":    ts,
    });
    world.pending_value = Some(json.to_string());
}

/// Verifies the signature is invalid after the purge token was tampered.
#[then("the signature verification should fail")]
fn signature_verification_fails(world: &mut VauchiWorld) {
    let (pk, sig_bytes, token, ts) = parse_purge_request(world);
    let message = purge_message(&pk, &token, ts);
    let sig = Signature::from_bytes(sig_bytes);
    assert!(
        !verify_signature(&pk, &message, &sig),
        "tampered purge request should fail signature verification"
    );
}

/// No-op: relay rejection is a network-level concern verified by relay tests.
#[then("the relay should reject the request")]
fn relay_rejects_request(_world: &mut VauchiWorld) {}

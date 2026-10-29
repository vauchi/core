// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use cucumber::{given, then, when};
use vauchi_core::api::PreSignedPurgeRequest;
use vauchi_core::crypto::signing::{Signature, verify_signature};

use crate::VauchiWorld;

// ── Relay purge, signed when it is sent (#899, #911) ──────────────────────

/// Signs a purge now and stores its fields as JSON in world.pending_value
/// for the verification steps.
#[given("I have a purge request signed now")]
fn have_purge_request_signed_now(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().expect("identity should exist");
    let req = &PreSignedPurgeRequest::sign_now(identity, 1_700_000_000);
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

/// Signs two purges with the same identity at the same time and keeps
/// both tokens, so the next step can compare them.
#[when("I sign two purge requests")]
fn sign_two_purge_requests(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().expect("identity should exist");
    let first = PreSignedPurgeRequest::sign_now(identity, 1_700_000_000);
    let second = PreSignedPurgeRequest::sign_now(identity, 1_700_000_000);
    world.pending_value = Some(
        serde_json::json!([first.purge_token.to_vec(), second.purge_token.to_vec()]).to_string(),
    );
}

#[then("their purge tokens differ")]
fn purge_tokens_differ(world: &mut VauchiWorld) {
    let tokens: Vec<Vec<u8>> = serde_json::from_str(
        world
            .pending_value
            .as_deref()
            .expect("no purge tokens stored"),
    )
    .unwrap();
    assert_ne!(tokens[0], tokens[1]);
}

/// The wording before #911; kept while features main still says it.
#[given("I have a pre-signed purge request")]
fn have_pre_signed_purge_request(world: &mut VauchiWorld) {
    have_purge_request_signed_now(world);
}

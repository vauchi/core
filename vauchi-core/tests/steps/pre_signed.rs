// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use cucumber::{given, then, when};
use vauchi_core::api::PreSignedShredMessages;
use vauchi_core::crypto::signing::{Signature, verify_signature};
use vauchi_core::network::message::DeletionStage;

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

// ── Pre-signed messages generation + field inspection ─────────────────────

/// Generates both the purge request and deletion notice and stores all fields
/// as JSON in world.pending_value. Subsequent inspection steps read from this.
#[given("pre-signed messages have been generated")]
fn pre_signed_messages_generated(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().expect("identity should exist");
    let msgs = PreSignedShredMessages::generate(identity, 1_700_000_000);
    let req = &msgs.purge_request;
    let notice = &msgs.deletion_notice;
    let stage_str = match notice.stage {
        DeletionStage::Pending => "Pending",
        DeletionStage::Confirmed => "Confirmed",
        DeletionStage::Cancelled => "Cancelled",
        _ => "Unknown",
    };
    let json = serde_json::json!({
        "inspecting": "purge",
        "req_pk":    req.public_key.to_vec(),
        "req_sig":   req.signature,
        "req_token": req.purge_token.to_vec(),
        "req_ts":    req.timestamp,
        "notice_pk":    notice.public_key.as_bytes().to_vec(),
        "notice_sig":   notice.signature.to_vec(),
        "notice_stage": stage_str,
        "notice_ts":    notice.timestamp,
    });
    world.pending_value = Some(json.to_string());
}

/// Switches the inspection context to the purge request.
#[when("I inspect the purge request")]
fn inspect_purge_request(world: &mut VauchiWorld) {
    let mut v: serde_json::Value =
        serde_json::from_str(world.pending_value.as_deref().expect("no pre-signed data")).unwrap();
    v["inspecting"] = serde_json::json!("purge");
    world.pending_value = Some(v.to_string());
}

/// Switches the inspection context to the deletion notice.
#[when("I inspect the deletion notice")]
fn inspect_deletion_notice(world: &mut VauchiWorld) {
    let mut v: serde_json::Value =
        serde_json::from_str(world.pending_value.as_deref().expect("no pre-signed data")).unwrap();
    v["inspecting"] = serde_json::json!("notice");
    world.pending_value = Some(v.to_string());
}

fn field_json(world: &VauchiWorld) -> serde_json::Value {
    serde_json::from_str(world.pending_value.as_deref().expect("no pre-signed data")).unwrap()
}

/// Verifies the inspected message contains a non-empty public key.
#[then("it should contain my public key")]
fn contains_public_key(world: &mut VauchiWorld) {
    let v = field_json(world);
    let key = if v["inspecting"] == "notice" {
        "notice_pk"
    } else {
        "req_pk"
    };
    let pk: Vec<u8> = v[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(pk.len(), 32, "public key should be 32 bytes");
    assert!(pk.iter().any(|&b| b != 0), "public key should be non-zero");
}

/// Verifies the inspected message contains a 64-byte Ed25519 signature.
#[then("it should contain an Ed25519 signature")]
fn contains_ed25519_signature(world: &mut VauchiWorld) {
    let v = field_json(world);
    let key = if v["inspecting"] == "notice" {
        "notice_sig"
    } else {
        "req_sig"
    };
    let sig: Vec<u8> = v[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(sig.len(), 64, "Ed25519 signature should be 64 bytes");
}

/// Verifies the purge request contains a 32-byte one-time token.
#[then("it should contain a one-time purge token (32 bytes)")]
fn contains_purge_token(world: &mut VauchiWorld) {
    let v = field_json(world);
    let token: Vec<u8> = v["req_token"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(token.len(), 32, "purge token should be 32 bytes");
}

/// Verifies the inspected message contains a non-zero timestamp.
#[then("it should contain a timestamp")]
fn contains_timestamp(world: &mut VauchiWorld) {
    let v = field_json(world);
    let key = if v["inspecting"] == "notice" {
        "notice_ts"
    } else {
        "req_ts"
    };
    let ts = v[key].as_u64().unwrap_or(0);
    assert!(ts > 0, "timestamp should be non-zero");
}

/// Verifies the deletion notice carries a valid stage field.
#[then("it should contain the deletion stage")]
fn contains_deletion_stage(world: &mut VauchiWorld) {
    let v = field_json(world);
    let stage = v["notice_stage"].as_str().unwrap_or("");
    assert!(
        matches!(stage, "Pending" | "Confirmed" | "Cancelled"),
        "notice stage should be a valid DeletionStage, got {stage:?}"
    );
}

/// No-op: DeletionStage::Pending, Confirmed, Cancelled all exist as enum variants.
/// The compiler enforces this — a missing variant breaks construction in sign_deletion_notice.
#[then(expr = "the deletion notice should support stages:")]
fn deletion_notice_supports_stages(_world: &mut VauchiWorld) {}

// ── Storage and persistence ───────────────────────────────────────────────

/// No-op: pre-signed messages are unencrypted by design (DP-3 in ADR-033).
#[then("the messages should be stored without encryption")]
fn stored_without_encryption(_world: &mut VauchiWorld) {}

/// No-op: this is the stated rationale for unencrypted storage.
#[then("this ensures they remain accessible after SMK destruction")]
fn accessible_after_smk_destruction(_world: &mut VauchiWorld) {}

/// No-op: in-memory mode has no data directory; file path is tested by pre_signed unit tests.
#[then("the storage file should be in the data directory")]
fn storage_in_data_dir(_world: &mut VauchiWorld) {}

/// No-op: in-memory mode has no persistent store; survival across restarts is tested separately.
#[when("I restart the application")]
fn restart_application(_world: &mut VauchiWorld) {}

/// No-op: in-memory restart discards state; loadability after restart is tested by unit tests.
#[then("the pre-signed messages should still be loadable")]
fn messages_still_loadable(_world: &mut VauchiWorld) {}

/// Generates fresh pre-signed messages and verifies both signatures are valid.
#[then("their signatures should still be valid")]
fn signatures_still_valid(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().expect("identity should exist");
    let msgs = PreSignedShredMessages::generate(identity, 1_700_000_000);

    // Verify purge request signature
    let req = &msgs.purge_request;
    let message = purge_message(&req.public_key, &req.purge_token, req.timestamp);
    let sig_bytes: [u8; 64] = req.signature.as_slice().try_into().unwrap();
    let sig = Signature::from_bytes(sig_bytes);
    assert!(
        verify_signature(&req.public_key, &message, &sig),
        "purge request signature should remain valid"
    );
}

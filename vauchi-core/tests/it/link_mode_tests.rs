// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Link mode URL generation, parsing, and escrow command production.

use base64::Engine as _;
use vauchi_core::exchange::command::ExchangeCommand;
use vauchi_core::exchange::link_mode::*;

// ================================================================
// URL generation
// ================================================================

// @internal
#[test]
fn generate_produces_valid_url() {
    let (init, _) = initiator_generate();
    assert!(
        init.url.starts_with("vauchi://exchange?"),
        "URL must use vauchi scheme"
    );
    assert!(init.url.contains("pk="), "URL must contain pk param");
    assert!(init.url.contains("n="), "URL must contain nonce param");
}

// @internal
#[test]
fn two_generations_produce_different_urls() {
    let (a, _) = initiator_generate();
    let (b, _) = initiator_generate();
    assert_ne!(a.url, b.url, "Each generation uses fresh randomness");
    assert_ne!(
        a.secret_key_bytes, b.secret_key_bytes,
        "Different secret keys"
    );
    assert_ne!(a.nonce, b.nonce, "Different nonces");
}

// @internal
#[test]
fn handshake_slot_is_64_hex_chars() {
    let (init, _) = initiator_generate();
    assert_eq!(init.handshake_slot.len(), 64);
    assert!(init.handshake_slot.chars().all(|c| c.is_ascii_hexdigit()));
}

// ================================================================
// URL parsing
// ================================================================

// @internal
#[test]
fn parse_valid_url_roundtrips() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).expect("valid URL must parse");
    assert_eq!(parsed.nonce, init.nonce, "nonce must roundtrip");
}

// @internal
#[test]
fn parse_rejects_wrong_scheme() {
    assert!(parse_link_url("https://example.com?pk=abc&n=def").is_none());
}

// @internal
#[test]
fn parse_rejects_missing_pk() {
    let n_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
    let url = format!("vauchi://exchange?n={n_b64}");
    assert!(parse_link_url(&url).is_none());
}

// @internal
#[test]
fn parse_rejects_short_pk() {
    let short = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 16]);
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
    let url = format!("vauchi://exchange?pk={short}&n={n}");
    assert!(parse_link_url(&url).is_none());
}

// @internal
#[test]
fn parse_rejects_invalid_base64() {
    let url = "vauchi://exchange?pk=!!!invalid!!!&n=!!!also!!!";
    assert!(parse_link_url(url).is_none());
}

// ================================================================
// Responder commands
// ================================================================

// @internal
#[test]
fn responder_produces_two_deposit_commands() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let (_, commands) = responder_respond(&parsed, b"encrypted_card".to_vec()).unwrap();
    assert_eq!(commands.len(), 2, "responder emits 2 deposits");

    // First command: handshake slot deposit (epk)
    assert!(matches!(
        &commands[0],
        ExchangeCommand::RelayEscrowDeposit { .. }
    ));
    // Second command: card deposit
    assert!(matches!(
        &commands[1],
        ExchangeCommand::RelayEscrowDeposit { .. }
    ));
}

// @internal
#[test]
fn responder_handshake_deposit_contains_32_byte_epk() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (_, commands) = responder_respond(&parsed, b"card".to_vec()).unwrap();

    if let ExchangeCommand::RelayEscrowDeposit { encrypted_card, .. } = &commands[0] {
        assert_eq!(
            encrypted_card.len(),
            32,
            "handshake deposit must be 32-byte epk"
        );
    } else {
        panic!("expected RelayEscrowDeposit");
    }
}

// ================================================================
// Initiator completion
// ================================================================

// @internal
#[test]
fn initiator_complete_produces_one_deposit() {
    // Simulate: responder generated an epk, initiator retrieves it
    let peer_epk = [0x42u8; 32]; // fake responder public key
    let (init, _) = initiator_generate();

    let (_, commands) = initiator_complete(
        &init.secret_key_bytes,
        &peer_epk,
        b"encrypted_card".to_vec(),
    )
    .unwrap();
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0],
        ExchangeCommand::RelayEscrowDeposit { .. }
    ));
}

// ================================================================
// End-to-end: initiator + responder derive same gate
// ================================================================

// @internal
#[test]
fn initiator_and_responder_derive_same_gate_hash() {
    // This is the critical security property: both sides agree on
    // the escrow gate despite using different code paths.

    // Step 1: Initiator generates URL
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    // Step 2: Responder responds (produces commands + keys)
    let (resp_keys, resp_commands) = responder_respond(&parsed, b"bob_card".to_vec()).unwrap();

    // Step 3: Extract responder's epk from handshake deposit
    let resp_epk: [u8; 32] =
        if let ExchangeCommand::RelayEscrowDeposit { encrypted_card, .. } = &resp_commands[0] {
            encrypted_card.as_slice().try_into().unwrap()
        } else {
            panic!("expected deposit");
        };

    // Step 4: Initiator completes with responder's epk
    let (init_keys, _) =
        initiator_complete(&init.secret_key_bytes, &resp_epk, b"alice_card".to_vec()).unwrap();

    // Critical assertion: same gate, swapped slots
    assert_eq!(
        init_keys.gate_hash, resp_keys.gate_hash,
        "Both sides must derive the same gate_hash"
    );
    assert_eq!(
        init_keys.our_slot, resp_keys.their_slot,
        "Initiator's our_slot = Responder's their_slot"
    );
    assert_eq!(
        init_keys.their_slot, resp_keys.our_slot,
        "Initiator's their_slot = Responder's our_slot"
    );
}

// ================================================================
// Security: handshake slot unrelated to gate hash
// ================================================================

// @internal
#[test]
fn handshake_slot_unrelated_to_gate_hash() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (keys, _) = responder_respond(&parsed, b"card".to_vec()).unwrap();

    assert_ne!(
        init.handshake_slot, keys.gate_hash,
        "handshake_slot must be unrelated to gate_hash (different derivation inputs)"
    );
}

// @internal
#[test]
fn handshake_slot_derived_from_nonce_not_secret() {
    // Same nonce → same handshake slot, regardless of keypair
    let nonce = [0xAA; 32];

    let pk_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x11; 32]);
    let n_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce);
    let url1 = format!("vauchi://exchange?pk={pk_b64}&n={n_b64}");

    let pk_b64_2 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x22; 32]);
    let url2 = format!("vauchi://exchange?pk={pk_b64_2}&n={n_b64}");

    let parsed1 = parse_link_url(&url1).unwrap();
    let parsed2 = parse_link_url(&url2).unwrap();

    // Both URLs have the same nonce, so handshake slots must match
    // (handshake_slot = H(nonce || "handshake"))
    // We can't call derive_handshake_slot directly (private), but we
    // can verify through the responder flow that the handshake deposits
    // go to the same gate.
    let (_, cmds1) = responder_respond(&parsed1, b"a".to_vec()).unwrap();
    let (_, cmds2) = responder_respond(&parsed2, b"b".to_vec()).unwrap();

    // Extract handshake gate_hash from first command of each
    let gate1 = match &cmds1[0] {
        ExchangeCommand::RelayEscrowDeposit { gate_hash, .. } => gate_hash.clone(),
        _ => panic!(),
    };
    let gate2 = match &cmds2[0] {
        ExchangeCommand::RelayEscrowDeposit { gate_hash, .. } => gate_hash.clone(),
        _ => panic!(),
    };

    assert_eq!(
        gate1, gate2,
        "Same nonce → same handshake slot, regardless of public key"
    );
}

// ================================================================
// Initiator presence deposit (D5 fix)
// ================================================================

// @internal
#[test]
fn initiator_generate_returns_presence_deposit_command() {
    let (_init, commands) = initiator_generate();
    assert_eq!(
        commands.len(),
        1,
        "initiator must emit exactly 1 presence deposit command"
    );
    assert!(matches!(
        &commands[0],
        ExchangeCommand::RelayEscrowDeposit { .. }
    ));
}

// @internal
#[test]
fn presence_deposit_targets_handshake_gate() {
    let (init, commands) = initiator_generate();
    if let ExchangeCommand::RelayEscrowDeposit { gate_hash, .. } = &commands[0] {
        let handshake_gate = hex::decode(&init.handshake_slot).unwrap();
        assert_eq!(
            gate_hash, &handshake_gate,
            "presence deposit must target the handshake gate"
        );
    } else {
        panic!("expected RelayEscrowDeposit");
    }
}

// @internal
#[test]
fn presence_deposit_uses_initiator_epk_as_blob() {
    let (_init, commands) = initiator_generate();
    if let ExchangeCommand::RelayEscrowDeposit { encrypted_card, .. } = &commands[0] {
        assert_eq!(
            encrypted_card.len(),
            32,
            "presence blob must be 32-byte public key"
        );
    } else {
        panic!("expected RelayEscrowDeposit");
    }
}

// @internal
#[test]
fn presence_and_responder_epk_use_different_slots() {
    let (init, init_cmds) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (_, resp_cmds) = responder_respond(&parsed, b"card".to_vec()).unwrap();

    // Extract slot hashes from handshake deposits
    let init_slot = match &init_cmds[0] {
        ExchangeCommand::RelayEscrowDeposit { slot_hash, .. } => slot_hash.clone(),
        _ => panic!(),
    };
    let resp_slot = match &resp_cmds[0] {
        ExchangeCommand::RelayEscrowDeposit { slot_hash, .. } => slot_hash.clone(),
        _ => panic!(),
    };

    assert_ne!(
        init_slot, resp_slot,
        "presence slot and epk slot must be different (distinct domain tags)"
    );
}

// @internal
#[test]
fn presence_slot_is_64_hex_chars() {
    let (init, _) = initiator_generate();
    assert_eq!(init.presence_slot.len(), 64);
    assert!(init.presence_slot.chars().all(|c| c.is_ascii_hexdigit()));
}

// ================================================================
// Security: small-order point rejection
// ================================================================

// @internal
#[test]
fn initiator_rejects_small_order_point() {
    let (init, _) = initiator_generate();
    // All-zeros is a small-order point on Curve25519
    let zero_key = [0u8; 32];
    let result = initiator_complete(&init.secret_key_bytes, &zero_key, b"card".to_vec());
    assert!(
        result.is_err(),
        "DH with small-order point must be rejected"
    );
}

// @internal
#[test]
fn responder_rejects_small_order_point_in_url() {
    // Craft a URL containing an all-zeros public key (small-order point)
    let zero_pk = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xAA; 32]);
    let url = format!("vauchi://exchange?pk={zero_pk}&n={nonce}");
    let parsed = parse_link_url(&url).unwrap();
    let result = responder_respond(&parsed, b"card".to_vec());
    assert!(
        result.is_err(),
        "Responder DH with small-order point must be rejected"
    );
}

// ================================================================
// parse_exchange_deep_link — deep-link consent gate parser
// (problem record 2026-04-25-deeplink-consent-orchestrator)
// ================================================================

// @internal
#[test]
fn parse_deep_link_accepts_canonical_query_form() {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical link_mode URL must parse");
    assert_eq!(*payload.nonce(), init.nonce);
    let pk_bytes = parse_link_url(&init.url).unwrap().initiator_public_key;
    assert_eq!(*payload.initiator_public_key(), pk_bytes);
}

// @internal
#[test]
fn parse_deep_link_round_trips_through_as_parsed() {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).unwrap();
    let direct = parse_link_url(&init.url).unwrap();
    assert_eq!(
        payload.as_parsed().initiator_public_key,
        direct.initiator_public_key
    );
    assert_eq!(payload.as_parsed().nonce, direct.nonce);
}

// @internal
#[test]
fn parse_deep_link_rejects_non_vauchi_scheme() {
    assert_eq!(
        parse_exchange_deep_link("https://exchange?pk=AAAA&n=BBBB"),
        Err(DeepLinkParseError::InvalidScheme),
    );
    assert_eq!(
        parse_exchange_deep_link("ftp://exchange?pk=AAAA&n=BBBB"),
        Err(DeepLinkParseError::InvalidScheme),
    );
}

// @internal
#[test]
fn parse_deep_link_rejects_wrong_host() {
    assert_eq!(
        parse_exchange_deep_link("vauchi://recover?pk=AAAA&n=BBBB"),
        Err(DeepLinkParseError::InvalidHost),
    );
    assert_eq!(
        parse_exchange_deep_link("vauchi://other?pk=AAAA&n=BBBB"),
        Err(DeepLinkParseError::InvalidHost),
    );
}

// @internal
#[test]
fn parse_deep_link_rejects_legacy_path_form() {
    // The defunct shape the original DeepLinkHandler.swift / .kt parsed.
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange/somepayload"),
        Err(DeepLinkParseError::LegacyPathForm),
    );
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange/abc/def"),
        Err(DeepLinkParseError::LegacyPathForm),
    );
}

// @internal
#[test]
fn parse_deep_link_rejects_malformed_query() {
    // Missing pk
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange?n=AAAA"),
        Err(DeepLinkParseError::MalformedQuery),
    );
    // Missing n
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange?pk=AAAA"),
        Err(DeepLinkParseError::MalformedQuery),
    );
    // Bad base64 in pk
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange?pk=!@#$&n=AAAA"),
        Err(DeepLinkParseError::MalformedQuery),
    );
    // Wrong-length pk (decoded != 32 bytes)
    let short = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 16]);
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
    assert_eq!(
        parse_exchange_deep_link(&format!("vauchi://exchange?pk={short}&n={nonce}")),
        Err(DeepLinkParseError::MalformedQuery),
    );
}

// @internal
#[test]
fn parse_deep_link_rejects_empty_and_whitespace() {
    assert_eq!(
        parse_exchange_deep_link(""),
        Err(DeepLinkParseError::InvalidScheme),
    );
    assert_eq!(
        parse_exchange_deep_link("   "),
        Err(DeepLinkParseError::InvalidScheme),
    );
}

// @internal
#[test]
fn parse_deep_link_rejects_no_query_separator() {
    // host=exchange, no `?` — shouldn't fall through to MalformedQuery silently
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange"),
        Err(DeepLinkParseError::MalformedQuery),
    );
}

// @internal
#[test]
fn parse_deep_link_tolerates_unknown_extra_params() {
    // The live parse_link_url ignores unknown params; the deep-link
    // wrapper preserves that behaviour so future protocol additions
    // don't break existing senders.
    let (init, _) = initiator_generate();
    let with_extra = format!("{}&extra=junk&future=v2", init.url);
    let payload = parse_exchange_deep_link(&with_extra).expect("extra params must not block parse");
    assert_eq!(*payload.nonce(), init.nonce);
}

// @internal
proptest::proptest! {
    /// Parser must never panic on arbitrary input and must never return
    /// `Ok` for any input that doesn't start with `vauchi://`.
    /// Property test (CC-04) covering scheme/host/payload fuzz.
    #[test]
    fn parse_deep_link_never_panics_or_misclassifies(input in ".*") {
        let result = parse_exchange_deep_link(&input);
        if let Ok(_) = result {
            // The parser only accepts canonical `vauchi://exchange?...`
            // shapes — anything else MUST be a typed error.
            proptest::prop_assert!(input.starts_with("vauchi://exchange?"),
                "parser accepted non-canonical input: {input:?}");
        }
    }
}

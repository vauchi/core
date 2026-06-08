// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Link mode URL generation, parsing, and escrow command production.

use base64::Engine as _;
use proptest::prelude::any;
use vauchi_core::Command;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::link_mode::*;

// ================================================================
// Card payload codec ([version][pubkey][card])
// ================================================================

// @internal
#[test]
fn card_payload_roundtrips_key_and_card() {
    let pubkey = [7u8; 32];
    let card = ContactCard::new("Alice");
    let bytes = serialize_card_payload(&pubkey, &card);

    let (parsed_key, parsed_card) =
        parse_card_payload(&bytes).expect("freshly serialized payload must parse");
    assert_eq!(parsed_key, pubkey, "public key must round-trip exactly");
    assert_eq!(
        parsed_card.id(),
        card.id(),
        "decoded card must be the same card (matched by id)"
    );
}

// @internal
#[test]
fn parse_card_payload_rejects_short_and_bad_version() {
    let err = parse_card_payload(&[0u8; 10]).expect_err("a 10-byte payload is too short");
    assert!(
        matches!(err, LinkModeError::MalformedCardPayload(_)),
        "short payload must be MalformedCardPayload, got {err:?}"
    );

    let mut bad_version = serialize_card_payload(&[1u8; 32], &ContactCard::new("Bob"));
    bad_version[0] = 0xFF;
    let err = parse_card_payload(&bad_version).expect_err("version 0xFF is unsupported");
    assert!(
        matches!(err, LinkModeError::MalformedCardPayload(_)),
        "bad version must be MalformedCardPayload, got {err:?}"
    );
}

// ================================================================
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
// ================================================================

// @internal
#[test]
fn responder_produces_two_deposit_commands() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let (_, commands) = responder_respond(&parsed, b"encrypted_card".to_vec()).unwrap();
    assert_eq!(commands.len(), 2, "responder emits 2 deposits");

    // First command: handshake slot deposit (epk)
    assert!(matches!(&commands[0], Command::RelayEscrowDeposit { .. }));
    // Second command: card deposit
    assert!(matches!(&commands[1], Command::RelayEscrowDeposit { .. }));
}

// @internal
#[test]
fn responder_handshake_deposit_contains_32_byte_epk() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (_, commands) = responder_respond(&parsed, b"card".to_vec()).unwrap();

    if let Command::RelayEscrowDeposit { encrypted_card, .. } = &commands[0] {
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
    assert!(matches!(&commands[0], Command::RelayEscrowDeposit { .. }));
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
        if let Command::RelayEscrowDeposit { encrypted_card, .. } = &resp_commands[0] {
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
        Command::RelayEscrowDeposit { gate_hash, .. } => gate_hash.clone(),
        _ => panic!(),
    };
    let gate2 = match &cmds2[0] {
        Command::RelayEscrowDeposit { gate_hash, .. } => gate_hash.clone(),
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
    assert!(matches!(&commands[0], Command::RelayEscrowDeposit { .. }));
}

// @internal
#[test]
fn presence_deposit_targets_handshake_gate() {
    let (init, commands) = initiator_generate();
    if let Command::RelayEscrowDeposit { gate_hash, .. } = &commands[0] {
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
    if let Command::RelayEscrowDeposit { encrypted_card, .. } = &commands[0] {
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
        Command::RelayEscrowDeposit { slot_hash, .. } => slot_hash.clone(),
        _ => panic!(),
    };
    let resp_slot = match &resp_cmds[0] {
        Command::RelayEscrowDeposit { slot_hash, .. } => slot_hash.clone(),
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
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange?n=AAAA"),
        Err(DeepLinkParseError::MalformedQuery),
    );
    assert_eq!(
        parse_exchange_deep_link("vauchi://exchange?pk=AAAA"),
        Err(DeepLinkParseError::MalformedQuery),
    );
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

// ================================================================
// Responder-side: responder_complete decrypts retrieved blob
// ================================================================

/// Round-trip: a payload encrypted with `EscrowKeys::encrypt_card` is
/// recovered byte-identically by `responder_complete`.
///
/// The crypto layer (RustCrypto AEAD) is already covered by `escrow.rs`
/// tests; this pin is specifically for the link-mode wrapper so the
/// cycle thread's happy path stays sound under refactoring.
// @internal
#[test]
fn responder_complete_round_trips_initiator_payload() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (keys, _) = responder_respond(&parsed, b"_".to_vec()).unwrap();

    let plaintext = b"Alice's serialized contact card";
    let ciphertext = keys.encrypt_card(plaintext).expect("encrypt");

    let recovered =
        responder_complete(&keys, &ciphertext).expect("responder_complete must round-trip");
    assert_eq!(recovered, plaintext);
}

/// Decrypting a truncated ciphertext returns a typed
/// `LinkModeError::CardCryptoFailed`, never a panic. Pins the error
/// path the cycle thread relies on to fire `on_failed(DecryptError)`.
// @internal
#[test]
fn responder_complete_rejects_truncated_ciphertext() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let (keys, _) = responder_respond(&parsed, b"_".to_vec()).unwrap();

    // Empty blob — fails the AEAD nonce-length check inside decrypt_card.
    let err = responder_complete(&keys, &[]).expect_err("empty ciphertext must error");
    assert!(
        matches!(err, LinkModeError::CardCryptoFailed(_)),
        "expected CardCryptoFailed, got {err:?}"
    );

    // 4-byte blob (smaller than the nonce) — same shape.
    let err = responder_complete(&keys, &[0u8; 4]).expect_err("undersized ciphertext must error");
    assert!(
        matches!(err, LinkModeError::CardCryptoFailed(_)),
        "expected CardCryptoFailed, got {err:?}"
    );
}

/// Decrypting a ciphertext encrypted with a *different* key returns
/// `LinkModeError::CardCryptoFailed`, not a silent garbled-bytes
/// success. Pins authenticated-encryption integrity.
// @internal
#[test]
fn responder_complete_rejects_wrong_key() {
    let (init_a, _) = initiator_generate();
    let parsed_a = parse_link_url(&init_a.url).unwrap();
    let (keys_a, _) = responder_respond(&parsed_a, b"_".to_vec()).unwrap();

    let (init_b, _) = initiator_generate();
    let parsed_b = parse_link_url(&init_b.url).unwrap();
    let (keys_b, _) = responder_respond(&parsed_b, b"_".to_vec()).unwrap();

    let ciphertext = keys_a.encrypt_card(b"secret").expect("encrypt");

    let err = responder_complete(&keys_b, &ciphertext)
        .expect_err("wrong-key decrypt must error, not garble silently");
    assert!(
        matches!(err, LinkModeError::CardCryptoFailed(_)),
        "expected CardCryptoFailed, got {err:?}"
    );
}

// ================================================================
// Responder-side: responder_respond_with_card_bytes encrypt-helper
// ================================================================

/// Production-ergonomic helper round-trips: caller supplies raw card
/// bytes; helper internally derives keys + encrypts; the second
/// deposit's blob can be decrypted back to the raw bytes via
/// `responder_complete` using the same returned keys.
// @internal
#[test]
fn responder_respond_with_card_bytes_round_trips_via_responder_complete() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let raw = b"alice serialized card";
    let (keys, commands) = responder_respond_with_card_bytes(&parsed, raw).unwrap();

    // The second command is the encrypted-card deposit. Pull the
    // ciphertext out and round-trip it back through responder_complete.
    let encrypted_blob = match &commands[1] {
        Command::RelayEscrowDeposit { encrypted_card, .. } => encrypted_card.clone(),
        other => panic!("expected RelayEscrowDeposit at index 1, got {other:?}"),
    };

    let recovered = responder_complete(&keys, &encrypted_blob).expect("decrypt round-trip");
    assert_eq!(recovered.as_slice(), raw);
}

// @internal
proptest::proptest! {
    /// `responder_complete` never panics on arbitrary blob bytes and
    /// always returns either Ok with the decrypted bytes (when keys
    /// match the ciphertext) or a typed `LinkModeError::CardCryptoFailed`.
    /// Property test (CC-04) covering blob fuzz.
    // @internal
    #[test]
    fn responder_complete_never_panics(blob in proptest::collection::vec(any::<u8>(), 0..512)) {
        let (init, _) = initiator_generate();
        let parsed = parse_link_url(&init.url).unwrap();
        let (keys, _) = responder_respond(&parsed, b"_".to_vec()).unwrap();

        let result = responder_complete(&keys, &blob);
        if let Err(e) = result {
            proptest::prop_assert!(
                matches!(e, LinkModeError::CardCryptoFailed(_)),
                "any decrypt failure must be CardCryptoFailed, got {e:?}"
            );
        }
    }
}

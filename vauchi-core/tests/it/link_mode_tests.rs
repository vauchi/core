// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for Link mode URL generation, parsing, and escrow command production.

use base64::Engine as _;
use proptest::prelude::any;
use vauchi_core::Command;
use vauchi_core::exchange::escrow::EscrowKeys;
use vauchi_core::exchange::link_mode::*;
use vauchi_core::exchange::link_pake::{LinkCode, LinkPakeError};

fn deposit_blob(command: &Command) -> Vec<u8> {
    match command {
        Command::RelayEscrowDeposit { encrypted_card, .. } => encrypted_card.clone(),
        other => panic!("expected a deposit, got {other:?}"),
    }
}

/// Runs both sides of a fresh link, the responder holding `typed_code` or, with
/// `None`, the code the sender shows. Returns the initiator's and the
/// responder's escrow keys.
fn run_both_sides(typed_code: Option<&LinkCode>) -> (EscrowKeys, EscrowKeys) {
    let (mut init, presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).expect("a generated link parses");
    let code = typed_code.unwrap_or(&init.code);
    let opening = responder_open(&parsed, code).expect("a generated link opens");
    let responder_blob = deposit_blob(&opening.deposit_command());
    let responder_keys = opening
        .finish(&deposit_blob(&presence[0]))
        .expect("the sender's deposit matches the link");
    let (_, initiator_keys) = initiator_derive_keys(
        &init.secret_key_bytes,
        init.pake.take().expect("a fresh initiation has its PAKE"),
        &responder_blob,
    )
    .expect("the responder's deposit is well formed");
    (initiator_keys, responder_keys)
}

fn responder_keys() -> EscrowKeys {
    run_both_sides(None).1
}

fn a_code_other_than_any_shown() -> LinkCode {
    LinkCode::parse("000000").expect("six digits")
}

// ================================================================
// Card payload codec ([version][pubkey][card])
// ================================================================

// ================================================================
// ================================================================

// @internal
#[test]
fn generate_produces_valid_url() {
    let (init, _) = initiator_generate();
    // An https link a non-user can open (vauchi/private#277); the payload
    // sits in the fragment, which browsers never send to the server.
    let (before_fragment, fragment) = init
        .url
        .split_once('#')
        .expect("the payload is in the fragment");
    assert_eq!(before_fragment, "https://vauchi.app/x");
    assert!(fragment.contains("pk="), "URL must contain pk param");
    assert!(fragment.contains("n="), "URL must contain nonce param");
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn name_param(url: &str) -> Option<String> {
    let fragment = url.split_once('#')?.1;
    let value = fragment.split('&').find_map(|p| p.strip_prefix("nm="))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .ok()?;
    String::from_utf8(bytes).ok()
}

// The landing page names the sender, read from the fragment only
// (vauchi/private#277, plan D1/D3).
// @internal
#[test]
fn named_link_carries_the_sender_name_and_still_parses() {
    let (init, _) = initiator_generate_named("Zoë Example");
    assert_eq!(name_param(&init.url).as_deref(), Some("Zoë Example"));
    let parsed = parse_link_url(&init.url).expect("named link parses");
    assert_eq!(parsed.nonce, init.nonce);
    assert!(parse_exchange_deep_link(&init.url).is_ok());
}

// The consent screen shows the name a link carries, marked as unchecked
// (vauchi/private#277, owner decision 2026-10-12).
// @internal
#[test]
fn a_named_link_hands_its_sender_name_to_the_consent_screen() {
    let (init, _) = initiator_generate_named("Zoë Example");

    let payload = parse_exchange_deep_link(&init.url).expect("named link parses");

    assert_eq!(payload.sender_name(), Some("Zoë Example"));
}

// @internal
#[test]
fn a_link_without_a_name_hands_on_none() {
    let (init, _) = initiator_generate_named("");

    let payload = parse_exchange_deep_link(&init.url).expect("unnamed link parses");

    assert_eq!(payload.sender_name(), None);
}

fn with_name_value(url: &str, value: &str) -> String {
    let base = url.split("&nm=").next().unwrap();
    format!("{base}&nm={value}")
}

// A name is attacker-controlled text: anything that cannot be shown as a
// plain name is dropped, and the link itself still opens (CC-14).
// @internal
#[test]
fn a_name_that_cannot_be_shown_safely_is_dropped_but_the_link_still_parses() {
    let (init, _) = initiator_generate_named("Zoë");
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let hostile = [
        ("not base64url", "!!!".to_owned()),
        ("invalid UTF-8", b64(&[0xff, 0xfe, 0x41])),
        ("empty", String::new()),
        ("only spaces", b64("   ".as_bytes())),
        ("over 64 bytes", b64("a".repeat(65).as_bytes())),
        ("newline", b64("Zoë\nYour bank".as_bytes())),
        ("right-to-left override", b64("Zoë\u{202E}knab".as_bytes())),
        ("zero-width joiner", b64("Zo\u{200D}ë".as_bytes())),
        ("byte order mark", b64("\u{FEFF}Zoë".as_bytes())),
    ];

    for (case, value) in hostile {
        let url = with_name_value(&init.url, &value);
        let payload = parse_exchange_deep_link(&url)
            .unwrap_or_else(|e| panic!("{case}: the link must still parse, got {e:?}"));
        assert_eq!(
            payload.sender_name(),
            None,
            "{case}: the name must be dropped"
        );
    }
}

// @internal
#[test]
fn unnamed_link_has_no_name_param() {
    let (init, _) = initiator_generate_named("");
    assert!(!init.url.contains("nm="), "{}", init.url);
}

// A long name is cut to 64 bytes on a character boundary, keeping the link
// and its QR short.
// @internal
#[test]
fn long_sender_name_is_cut_on_a_character_boundary() {
    let name = "é".repeat(50); // 100 bytes
    let (init, _) = initiator_generate_named(&name);
    let carried = name_param(&init.url).expect("name present and valid UTF-8");
    assert_eq!(carried, "é".repeat(32));
}

// The landing page's Open button hands this form to an installed app.
// @internal
#[test]
fn app_form_links_parse() {
    let (pk, n) = ([7u8; 32], [9u8; 32]);
    let link = format!("vauchi://exchange?pk={}&n={}&v=2", b64(&pk), b64(&n));
    let parsed = parse_link_url(&link).expect("app-form link parses");
    assert_eq!(parsed.initiator_public_key, pk);
    assert_eq!(parsed.nonce, n);
    assert!(parse_exchange_deep_link(&link).is_ok());
}

// A link with no code behind it would keep the attack the code removes: the
// first person to open it becomes a contact (#727).
// @claim: S9
// @internal
#[test]
fn a_link_of_no_supported_version_is_refused() {
    let params = format!("pk={}&n={}", b64(&[7u8; 32]), b64(&[9u8; 32]));
    for suffix in ["", "&v=", "&v=1", "&v=3", "&v=2x", "&v=20", "&V=2"] {
        for prefix in ["https://vauchi.app/x#", "vauchi://exchange?"] {
            let link = format!("{prefix}{params}{suffix}");
            assert_eq!(parse_link_url(&link), None, "{link}");
            assert_eq!(
                parse_exchange_deep_link(&link),
                Err(DeepLinkParseError::MalformedQuery),
                "{link}"
            );
        }
    }
}

// @claim: S9
// @internal
#[test]
fn the_link_holds_nothing_but_its_key_nonce_version_and_name() {
    let (init, _) = initiator_generate_named("Alice");
    let (_, fragment) = init.url.split_once('#').expect("fragment");
    let names: Vec<&str> = fragment
        .split('&')
        .filter_map(|param| param.split_once('=').map(|(name, _)| name))
        .collect();
    assert_eq!(names, ["pk", "n", "v", "nm"]);
}

// @internal
#[test]
fn https_link_opens_through_the_deep_link_parser() {
    let (init, _) = initiator_generate();
    assert!(parse_exchange_deep_link(&init.url).is_ok());
}

// The payload must stay out of anything a server sees (plan D1), and only
// vauchi.app/x is ours.
// @internal
#[test]
fn https_links_outside_the_fragment_or_off_domain_are_rejected() {
    let params = format!("pk={}&n={}&v=2", b64(&[1u8; 32]), b64(&[2u8; 32]));
    for url in [
        format!("https://vauchi.app/x?{params}"),
        format!("https://evil.example/x#{params}"),
        format!("https://vauchi.app/y#{params}"),
        format!("http://vauchi.app/x#{params}"),
        "https://vauchi.app/x#".to_string(),
        "https://vauchi.app/x".to_string(),
    ] {
        assert!(parse_link_url(&url).is_none(), "{url} must not parse");
        assert!(
            parse_exchange_deep_link(&url).is_err(),
            "{url} must not open"
        );
    }
}

proptest::proptest! {
    // Both forms carry the same key and nonce (CC-04).
    // @internal
    #[test]
    fn both_link_forms_roundtrip(pk in any::<[u8; 32]>(), n in any::<[u8; 32]>()) {
        let params = format!("pk={}&n={}&v=2", b64(&pk), b64(&n));
        for url in [format!("https://vauchi.app/x#{params}"), format!("vauchi://exchange?{params}")] {
            let parsed = parse_link_url(&url).expect("well-formed link parses");
            proptest::prop_assert_eq!(parsed.initiator_public_key, pk);
            proptest::prop_assert_eq!(parsed.nonce, n);
        }
    }
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
    let url = format!("vauchi://exchange?pk={short}&n={n}&v=2");
    assert!(parse_link_url(&url).is_none());
}

// @internal
#[test]
fn parse_rejects_invalid_base64() {
    let url = "vauchi://exchange?pk=!!!invalid!!!&n=!!!also!!!&v=2";
    assert!(parse_link_url(url).is_none());
}

// ================================================================
// ================================================================

// The responder deposits before it holds the escrow keys, because the relay
// serves the sender's deposit only once the gate holds two slots; its card
// waits for the keys (#727).
// @internal
#[test]
fn the_responder_deposits_its_key_and_pake_message_in_the_handshake_gate() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let opening = responder_open(&parsed, &init.code).unwrap();

    match opening.deposit_command() {
        Command::RelayEscrowDeposit {
            gate_hash,
            slot_hash,
            encrypted_card,
            ..
        } => {
            assert_eq!(gate_hash, handshake_gate(&parsed.nonce));
            assert_eq!(slot_hash, opening.epk_slot());
            assert_eq!(encrypted_card.len(), 32 + 33);
            assert_eq!(&encrypted_card[..32], &opening.binding().responder_epk);
        }
        other => panic!("expected a deposit, got {other:?}"),
    }
}

// ================================================================
// ================================================================

// @internal
#[test]
fn an_escrow_deposit_goes_to_our_slot_in_the_escrow_gate() {
    let keys = responder_keys();

    let commands = build_escrow_deposit(&keys, b"encrypted_card".to_vec());

    match &commands[..] {
        [
            Command::RelayEscrowDeposit {
                gate_hash,
                slot_hash,
                encrypted_card,
                ..
            },
        ] => {
            assert_eq!(*gate_hash, hex::decode(&keys.gate_hash).unwrap());
            assert_eq!(*slot_hash, hex::decode(&keys.our_slot).unwrap());
            assert_eq!(encrypted_card, b"encrypted_card");
        }
        other => panic!("expected one deposit, got {other:?}"),
    }
}

// ================================================================
// End-to-end: initiator + responder derive same gate
// ================================================================

// @internal
#[test]
fn initiator_and_responder_derive_same_gate_hash() {
    // This is the critical security property: both sides agree on
    // the escrow gate despite using different code paths.
    let (init_keys, resp_keys) = run_both_sides(None);

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

// Whoever holds only the link cannot reach the sender's card: without the code
// the two sides sit in different gates, and neither key opens the other's
// deposit (#727, SC-74).
// @claim: S9
// @internal
#[test]
fn a_wrong_code_leaves_the_two_sides_in_different_gates() {
    let (init_keys, resp_keys) = run_both_sides(Some(&a_code_other_than_any_shown()));

    assert_ne!(init_keys.gate_hash, resp_keys.gate_hash);
    assert_ne!(init_keys.our_slot, resp_keys.their_slot);

    let from_initiator = init_keys.encrypt_card(b"alice's card").expect("encrypt");
    assert!(matches!(
        responder_complete(&resp_keys, &from_initiator),
        Err(LinkModeError::CardCryptoFailed(_))
    ));
    let from_responder = resp_keys.encrypt_card(b"bob's card").expect("encrypt");
    assert!(init_keys.decrypt_card(&from_responder).is_err());
}

// @internal
#[test]
fn a_person_typing_the_code_as_grouped_on_screen_reaches_the_same_gate() {
    let (mut init, presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let typed = LinkCode::parse(&init.code.grouped()).expect("the shown form parses");

    let opening = responder_open(&parsed, &typed).unwrap();
    let blob = deposit_blob(&opening.deposit_command());
    let resp_keys = opening.finish(&deposit_blob(&presence[0])).unwrap();
    let (_, init_keys) =
        initiator_derive_keys(&init.secret_key_bytes, init.pake.take().unwrap(), &blob).unwrap();

    assert_eq!(init_keys.gate_hash, resp_keys.gate_hash);
}

// A relay answering with another exchange's deposit must not get the responder
// to run the PAKE against a key the link does not name (DC-01).
// @internal
#[test]
fn a_sender_deposit_naming_another_key_than_the_link_is_refused() {
    let (init, _) = initiator_generate();
    let (_, foreign_presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let result = responder_open(&parsed, &init.code)
        .unwrap()
        .finish(&deposit_blob(&foreign_presence[0]));

    assert!(matches!(result, Err(LinkModeError::PresenceKeyMismatch)));
}

// Symmetric SPAKE2 would complete against itself: a relay handing the sender
// its own deposit back must not get it past the handshake without a code.
// @internal
#[test]
fn the_initiator_refuses_its_own_deposit_back() {
    let (mut init, presence) = initiator_generate();

    let result = initiator_derive_keys(
        &init.secret_key_bytes,
        init.pake.take().unwrap(),
        &deposit_blob(&presence[0]),
    );

    assert!(matches!(
        result,
        Err(LinkModeError::Pake(LinkPakeError::ReflectedMessage))
    ));
}

proptest::proptest! {
    // CC-14: the other side's deposit is untrusted. Anything but its exact
    // length is refused before any crypto runs.
    // @internal
    #[test]
    fn a_handshake_deposit_of_the_wrong_length_is_refused(
        blob in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        proptest::prop_assume!(blob.len() != 32 + 33);
        let (mut init, _) = initiator_generate();
        let parsed = parse_link_url(&init.url).unwrap();

        let from_responder = initiator_derive_keys(
            &init.secret_key_bytes,
            init.pake.take().unwrap(),
            &blob,
        );
        let from_sender = responder_open(&parsed, &init.code).unwrap().finish(&blob);

        let refused_with_its_length = |result: Result<_, LinkModeError>| {
            result.is_err_and(|error| {
                matches!(
                    error,
                    LinkModeError::MalformedHandshakeBlob { received } if received == blob.len()
                )
            })
        };
        proptest::prop_assert!(refused_with_its_length(from_responder.map(|_| ())));
        proptest::prop_assert!(refused_with_its_length(from_sender.map(|_| ())));
    }

    // CC-14: the PAKE message inside a well-sized deposit is untrusted too. It
    // either fails as invalid or yields keys nobody else holds; it never panics.
    // @internal
    #[test]
    fn a_garbage_pake_message_never_reaches_the_senders_gate(
        message in proptest::collection::vec(any::<u8>(), 33..=33),
        pk in any::<[u8; 32]>(),
    ) {
        let (mut init, presence) = initiator_generate();
        let parsed = parse_link_url(&init.url).unwrap();
        let mut blob = pk.to_vec();
        blob.extend_from_slice(&message);

        let genuine = responder_open(&parsed, &init.code)
            .unwrap()
            .finish(&deposit_blob(&presence[0]))
            .unwrap();
        let forged = initiator_derive_keys(
            &init.secret_key_bytes,
            init.pake.take().unwrap(),
            &blob,
        );

        match forged {
            Ok((_, keys)) => proptest::prop_assert_ne!(&keys.gate_hash, &genuine.gate_hash),
            Err(error) => proptest::prop_assert!(matches!(
                error,
                LinkModeError::Pake(LinkPakeError::InvalidMessage)
                    | LinkModeError::NonContributoryDh
            ), "{error:?}"),
        }
    }
}

// ================================================================
// Security: handshake slot unrelated to gate hash
// ================================================================

// @internal
#[test]
fn handshake_slot_unrelated_to_gate_hash() {
    let (init, _) = initiator_generate();
    let keys = responder_keys();

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
    let url1 = format!("vauchi://exchange?pk={pk_b64}&n={n_b64}&v=2");

    let pk_b64_2 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x22; 32]);
    let url2 = format!("vauchi://exchange?pk={pk_b64_2}&n={n_b64}&v=2");

    let parsed1 = parse_link_url(&url1).unwrap();
    let parsed2 = parse_link_url(&url2).unwrap();

    // Both URLs have the same nonce, so handshake slots must match
    // (handshake_slot = H(nonce || "handshake"))
    // We can't call derive_handshake_slot directly (private), but we
    // can verify through the responder flow that the handshake deposits
    // go to the same gate.
    let code = a_code_other_than_any_shown();
    let gate1 = responder_open(&parsed1, &code).unwrap().handshake_gate();
    let gate2 = responder_open(&parsed2, &code).unwrap().handshake_gate();

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
    let (init, commands) = initiator_generate();
    let blob = deposit_blob(&commands[0]);
    assert_eq!(blob.len(), 32 + 33, "presence blob is pk_i || msg_i");
    assert_eq!(&blob[..32], &init.public_key);
}

// @internal
#[test]
fn presence_and_responder_epk_use_different_slots() {
    let (init, init_cmds) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let resp_slot = responder_open(&parsed, &init.code).unwrap().epk_slot();
    let init_slot = match &init_cmds[0] {
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
    let (mut init, presence) = initiator_generate();
    // All-zeros is a small-order point on Curve25519
    let mut blob = vec![0u8; 32];
    blob.extend_from_slice(&deposit_blob(&presence[0])[32..]);
    let result = initiator_derive_keys(&init.secret_key_bytes, init.pake.take().unwrap(), &blob);
    assert!(
        matches!(result, Err(LinkModeError::NonContributoryDh)),
        "DH with small-order point must be rejected"
    );
}

// @internal
#[test]
fn responder_rejects_small_order_point_in_url() {
    // Craft a URL containing an all-zeros public key (small-order point)
    let zero_pk = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 32]);
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xAA; 32]);
    let url = format!("vauchi://exchange?pk={zero_pk}&n={nonce}&v=2");
    let parsed = parse_link_url(&url).unwrap();
    let result = responder_open(&parsed, &a_code_other_than_any_shown());
    assert!(
        matches!(result, Err(LinkModeError::NonContributoryDh)),
        "Responder DH with small-order point must be rejected before any deposit"
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
    // https is the link form since #277, but only for vauchi.app/x.
    assert_eq!(
        parse_exchange_deep_link("https://exchange?pk=AAAA&n=BBBB"),
        Err(DeepLinkParseError::InvalidHost),
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
        parse_exchange_deep_link(&format!("vauchi://exchange?pk={short}&n={nonce}&v=2")),
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
    /// `Ok` for any input outside the two canonical link forms.
    /// Property test (CC-04) covering scheme/host/payload fuzz.
    #[test]
    fn parse_deep_link_never_panics_or_misclassifies(input in ".*") {
        let result = parse_exchange_deep_link(&input);
        if result.is_ok() {
            // The parser only accepts `https://vauchi.app/x#...` and the
            // legacy `vauchi://exchange?...` — anything else MUST be a
            // typed error.
            proptest::prop_assert!(
                input.starts_with("https://vauchi.app/x#")
                    || input.starts_with("vauchi://exchange?"),
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
/// The two parties never share an `EscrowKeys`: each derives its own from
/// its half of the ECDH. Every other round-trip test in this file encrypts
/// and decrypts with the *same* keys object, so none of them can observe a
/// derivation asymmetry — which is how link-mode exchange reached hardware
/// with the initiator completing while the responder failed to decrypt
/// (2026-08-14-link-exchange-responder-cannot-decrypt-the-card).
// @scenario: link_exchange :: the responder decrypts the initiator's deposit
#[test]
fn responder_decrypts_a_card_encrypted_with_independently_derived_initiator_keys() {
    let (initiator_keys, responder_keys) = run_both_sides(None);

    let plaintext = b"alice serialized card";
    let ciphertext = initiator_keys
        .encrypt_card(plaintext)
        .expect("encrypting with the initiator's own keys must succeed");

    let recovered = responder_complete(&responder_keys, &ciphertext).expect(
        "the responder must decrypt what the initiator encrypted — both sides \
         derive from the same ECDH and code, so a failure here means the two derivations \
         disagree and only the initiator ever completes",
    );
    assert_eq!(recovered.as_slice(), plaintext);

    // Symmetry is the actual contract, so assert the other direction too.
    let responder_ciphertext = responder_keys
        .encrypt_card(b"bob serialized card")
        .expect("encrypting with the responder's own keys must succeed");
    let initiator_recovered = initiator_keys
        .decrypt_card(&responder_ciphertext)
        .expect("the initiator must decrypt what the responder encrypted");
    assert_eq!(initiator_recovered.as_slice(), b"bob serialized card");
}

// @internal
#[test]
fn responder_complete_round_trips_initiator_payload() {
    let keys = responder_keys();

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
    let keys = responder_keys();

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
    let keys_a = responder_keys();

    let keys_b = responder_keys();

    let ciphertext = keys_a.encrypt_card(b"secret").expect("encrypt");

    let err = responder_complete(&keys_b, &ciphertext)
        .expect_err("wrong-key decrypt must error, not garble silently");
    assert!(
        matches!(err, LinkModeError::CardCryptoFailed(_)),
        "expected CardCryptoFailed, got {err:?}"
    );
}

// ================================================================
// Responder-side: the binding its card is signed for
// ================================================================

/// The binding both cards are signed for is the link's nonce and key plus the
/// responder's fresh epk (#942).
// @internal
#[test]
fn the_responders_binding_is_the_link_and_its_own_epk() {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();

    let opening = responder_open(&parsed, &init.code).unwrap();

    let epk: [u8; 32] = deposit_blob(&opening.deposit_command())[..32]
        .try_into()
        .expect("an epk is 32 bytes");
    assert_eq!(
        *opening.binding(),
        LinkExchangeBinding {
            nonce: parsed.nonce,
            initiator_epk: parsed.initiator_public_key,
            responder_epk: epk,
        }
    );
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
        let keys = responder_keys();

        let result = responder_complete(&keys, &blob);
        if let Err(e) = result {
            proptest::prop_assert!(
                matches!(e, LinkModeError::CardCryptoFailed(_)),
                "any decrypt failure must be CardCryptoFailed, got {e:?}"
            );
        }
    }
}

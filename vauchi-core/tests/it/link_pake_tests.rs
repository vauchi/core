// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The link exchange's PAKE (#727, SC-74): a short code both people hold
//! yields one shared key, and any other code does not.

use proptest::prelude::*;
use vauchi_core::exchange::link_pake::{LINK_PAKE_MESSAGE_LEN, LinkCode, LinkPake, LinkPakeError};

const NONCE: [u8; 32] = [0x11; 32];
const PK_I: [u8; 32] = [0x22; 32];

fn code(digits: &str) -> LinkCode {
    LinkCode::parse(digits).unwrap()
}

fn run(a: &LinkCode, b: &LinkCode) -> ([u8; 32], [u8; 32]) {
    let (side_a, msg_a) = LinkPake::start(a, &NONCE, &PK_I);
    let (side_b, msg_b) = LinkPake::start(b, &NONCE, &PK_I);
    (
        *side_a.finish(&msg_b).unwrap(),
        *side_b.finish(&msg_a).unwrap(),
    )
}

// @internal
#[test]
fn the_same_code_gives_both_sides_the_same_key() {
    let (a, b) = run(&code("123456"), &code("123456"));
    assert_eq!(a, b);
    assert_ne!(a, [0u8; 32]);
}

// @internal
#[test]
fn a_different_code_gives_different_keys() {
    let (a, b) = run(&code("123456"), &code("123457"));
    assert_ne!(a, b);
}

// @internal
#[test]
fn the_key_is_bound_to_this_link() {
    let shared = code("123456");
    let (side_a, msg_a) = LinkPake::start(&shared, &NONCE, &PK_I);
    let (side_b, msg_b) = LinkPake::start(&shared, &[0x33; 32], &PK_I);
    let (side_c, msg_c) = LinkPake::start(&shared, &NONCE, &[0x44; 32]);

    let a = side_a.finish(&msg_b).unwrap();
    let b = side_b.finish(&msg_a).unwrap();
    assert_ne!(*a, *b, "another nonce is another link");

    let (side_d, msg_d) = LinkPake::start(&shared, &NONCE, &PK_I);
    let c = side_c.finish(&msg_d).unwrap();
    let d = side_d.finish(&msg_c).unwrap();
    assert_ne!(*c, *d, "another initiator key is another link");
}

// @internal
#[test]
fn a_message_has_the_length_the_blobs_rely_on() {
    let (_, message) = LinkPake::start(&code("000000"), &NONCE, &PK_I);
    assert_eq!(message.len(), LINK_PAKE_MESSAGE_LEN);
}

// @internal
#[test]
fn our_own_message_reflected_back_is_refused() {
    let (side, message) = LinkPake::start(&code("123456"), &NONCE, &PK_I);
    assert!(matches!(
        side.finish(&message),
        Err(LinkPakeError::ReflectedMessage)
    ));
}

// @internal
#[test]
fn malformed_peer_messages_are_refused() {
    for bad in [
        vec![],
        vec![0u8; 1],
        vec![0u8; LINK_PAKE_MESSAGE_LEN - 1],
        vec![0u8; LINK_PAKE_MESSAGE_LEN + 1],
        vec![0xFF; LINK_PAKE_MESSAGE_LEN],
        vec![b'S'; 4096],
    ] {
        let (side, _) = LinkPake::start(&code("123456"), &NONCE, &PK_I);
        assert!(
            matches!(side.finish(&bad), Err(LinkPakeError::InvalidMessage)),
            "accepted {} bytes",
            bad.len()
        );
    }
}

// @internal
#[test]
fn a_typed_code_accepts_the_separators_people_use() {
    for typed in ["123456", "123 456", "123-456", " 123456 "] {
        assert_eq!(LinkCode::parse(typed).unwrap().as_str(), "123456");
    }
}

// @internal
#[test]
fn a_code_that_is_not_six_digits_is_refused() {
    for typed in [
        "",
        " ",
        "12345",
        "1234567",
        "12345a",
        "abcdef",
        "١٢٣٤٥٦",
        "12345\u{0}",
        "1234 5",
        "123456789012345678901234567890",
    ] {
        assert!(
            matches!(LinkCode::parse(typed), Err(LinkPakeError::InvalidCode)),
            "accepted {typed:?}"
        );
    }
}

// @internal
#[test]
fn a_generated_code_is_six_digits_and_varies() {
    let codes: Vec<String> = (0..64)
        .map(|_| LinkCode::generate().as_str().to_string())
        .collect();
    for generated in &codes {
        assert_eq!(generated.len(), 6);
        assert!(generated.bytes().all(|b| b.is_ascii_digit()), "{generated}");
    }
    let first = &codes[0];
    assert!(codes.iter().any(|other| other != first), "all codes equal");
}

// @internal
#[test]
fn a_code_never_appears_in_debug_output() {
    let shown = format!("{:?}", code("987654"));
    assert!(!shown.contains("987654"), "{shown}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal
    #[test]
    fn any_two_codes_agree_exactly_when_they_are_equal(a in 0u32..1_000_000, b in 0u32..1_000_000) {
        let (key_a, key_b) = run(&code(&format!("{a:06}")), &code(&format!("{b:06}")));
        prop_assert_eq!(key_a == key_b, a == b);
    }

    // @internal
    #[test]
    fn any_bytes_as_a_peer_message_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let (side, _) = LinkPake::start(&code("123456"), &NONCE, &PK_I);
        let _outcome = side.finish(&bytes);
    }

    // @internal
    #[test]
    fn any_text_as_a_code_never_panics(typed in ".{0,32}") {
        let _outcome = LinkCode::parse(&typed);
    }
}

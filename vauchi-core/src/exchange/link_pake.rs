// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The link exchange's out-of-band confirmation (#727, SC-74).
//!
//! The sender gives the opener a short code over another channel. Symmetric
//! SPAKE2 turns it into a shared key, so whoever holds only the link gets one
//! guess and cannot test guesses offline.
//!
//! `spake2` comes from the fork in `vauchi/utils/spake2`, which wipes its
//! secrets on drop: the ADR-002 amendment of 2026-11-06.

use std::fmt;

use spake2::{Ed25519Group, Identity, Password, Spake2};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

// The crates.io 0.4.0 holds the same API without the wiping, so resolving to it
// would build and run while leaving the code and scalar in memory. This fails
// that build in every consumer, whose lock files Core's tests cannot read.
const _: fn() = || {
    fn wipes_on_drop<T: ZeroizeOnDrop>() {}
    wipes_on_drop::<Spake2<Ed25519Group>>();
    wipes_on_drop::<Password>();
};

const IDENTITY_DOMAIN: &[u8] = b"vauchi-link-pake-v1";
const CODE_DIGITS: usize = 6;
const CODE_SPACE: u32 = 1_000_000;
const KEY_LEN: usize = 32;

/// Length of a SPAKE2 message over `Ed25519Group`: a side byte and a point.
pub const LINK_PAKE_MESSAGE_LEN: usize = 33;

/// Why a code or a peer's message was refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkPakeError {
    /// The typed code is not six ASCII digits.
    #[error("the code must be six digits")]
    InvalidCode,
    /// The peer's message is not a valid SPAKE2 message.
    #[error("invalid PAKE message")]
    InvalidMessage,
    /// The peer's message is our own: symmetric SPAKE2 would complete against
    /// itself with no second party.
    #[error("the PAKE message is our own")]
    ReflectedMessage,
}

/// The short code the two people share.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct LinkCode(String);

impl LinkCode {
    /// Draws a code uniformly from the whole code space.
    pub fn generate() -> Self {
        let unbiased_below = u32::MAX - (u32::MAX % CODE_SPACE);
        loop {
            let candidate = u32::from_be_bytes(crate::crypto::random_bytes::<4>());
            if candidate < unbiased_below {
                return Self(format!("{:06}", candidate % CODE_SPACE));
            }
        }
    }

    /// Reads a code as a person typed it: `123456`, `123 456` or `123-456`.
    pub fn parse(typed: &str) -> Result<Self, LinkPakeError> {
        let typed = typed.trim();
        let bytes = typed.as_bytes();
        let digits: Zeroizing<String> = match bytes.len() {
            CODE_DIGITS => Zeroizing::new(typed.to_string()),
            len if len == CODE_DIGITS + 1
                && matches!(bytes[CODE_DIGITS / 2], b' ' | b'-')
                && typed.is_char_boundary(CODE_DIGITS / 2) =>
            {
                Zeroizing::new(typed.replacen([' ', '-'], "", 1))
            }
            _ => return Err(LinkPakeError::InvalidCode),
        };
        if digits.len() == CODE_DIGITS && digits.bytes().all(|b| b.is_ascii_digit()) {
            Ok(Self(digits.to_string()))
        } else {
            Err(LinkPakeError::InvalidCode)
        }
    }

    /// The digits, for display to the person who passes the code on.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LinkCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LinkCode(redacted)")
    }
}

/// One side of a run, waiting for the other side's message.
pub struct LinkPake {
    state: Spake2<Ed25519Group>,
    our_message: Vec<u8>,
}

impl LinkPake {
    /// Starts a run bound to this link, returning the message to deposit.
    pub fn start(
        code: &LinkCode,
        nonce: &[u8; 32],
        initiator_public_key: &[u8; 32],
    ) -> (Self, Vec<u8>) {
        let mut identity = Vec::with_capacity(IDENTITY_DOMAIN.len() + 64);
        identity.extend_from_slice(IDENTITY_DOMAIN);
        identity.extend_from_slice(nonce);
        identity.extend_from_slice(initiator_public_key);
        let (state, message) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(code.as_str()),
            &Identity::new(&identity),
        );
        (
            Self {
                state,
                our_message: message.clone(),
            },
            message,
        )
    }

    /// Finishes the run. A wrong code is not an error here: it yields a key
    /// the other side does not hold, which is what keeps it from being a test.
    pub fn finish(self, peer_message: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, LinkPakeError> {
        if peer_message == self.our_message {
            return Err(LinkPakeError::ReflectedMessage);
        }
        let key = Zeroizing::new(
            self.state
                .finish(peer_message)
                .map_err(|_| LinkPakeError::InvalidMessage)?,
        );
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        if key.len() != KEY_LEN {
            return Err(LinkPakeError::InvalidMessage);
        }
        out.copy_from_slice(&key);
        Ok(out)
    }
}

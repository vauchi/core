// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tag a confirm or done frame carries (vauchi/private#1217).
//!
//! Keyed from the transport key, so only a holder of one of the session's
//! two ephemeral secrets can compute it: a payload recorded in an earlier
//! exchange and replayed here cannot be confirmed. It covers a label (so a
//! confirm cannot be read as a done), both sides' session ids, ephemerals
//! and commitment hashes in sender-then-receiver order (a frame from another
//! binding cannot verify), and both payloads. 32 bytes: the frame stays the
//! size of the old final frame (QR version 5 at distance).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::crypto::kdf::HKDF;

const KEY_INFO: &[u8] = b"vauchi-multistage-confirm-v3";

/// What the tag says: "I accepted you" (confirm) or "I saved you" (done).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagLabel {
    Confirm,
    Done,
}

impl TagLabel {
    fn bytes(self) -> &'static [u8] {
        match self {
            TagLabel::Confirm => b"CNF",
            TagLabel::Done => b"DON",
        }
    }
}

/// One side of the session as the tag covers it.
pub struct Side<'a> {
    pub session_id: &'a [u8; 16],
    pub ephemeral: &'a [u8; 32],
    pub commitment_hash: &'a [u8; 32],
    pub payload: &'a [u8],
}

pub fn tag(
    transport_key: &[u8; 32],
    label: TagLabel,
    sender: &Side<'_>,
    receiver: &Side<'_>,
) -> [u8; 32] {
    let key = HKDF::derive_key(None, transport_key, KEY_INFO);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref())
        .expect("HMAC-SHA256 accepts a 32-byte key");
    mac.update(label.bytes());
    for side in [sender, receiver] {
        mac.update(side.session_id);
    }
    for side in [sender, receiver] {
        mac.update(side.ephemeral);
    }
    for side in [sender, receiver] {
        mac.update(side.commitment_hash);
    }
    for side in [sender, receiver] {
        mac.update(&Sha256::digest(side.payload));
    }
    mac.finalize().into_bytes().into()
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tag a confirm or done frame carries (vauchi/private#1217), and what
//! it states about the sender's contact list (vauchi/private#1285).
//!
//! Keyed from the transport key, so only a holder of one of the session's
//! two ephemeral secrets can compute it: a payload recorded in an earlier
//! exchange and replayed here cannot be confirmed. It covers a label, both
//! sides' session ids, ephemerals and commitment hashes in
//! sender-then-receiver order (a frame from another binding cannot verify),
//! and both payloads. 32 bytes: the frame stays the size of the old final
//! frame (QR version 5 at distance).
//!
//! The label is the statement. The frame does not carry it in the clear:
//! the reader tries each label its kind of frame allows, so a camera that
//! is not the session's peer cannot tell whether the sender holds the
//! reader as a contact.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::crypto::kdf::HKDF;

const KEY_INFO: &[u8] = b"vauchi-multistage-confirm-v4";

/// Whether the sender already holds the receiver as a contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holding {
    Peer,
    Nothing,
}

/// How the exchange ended on the sender's phone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// It stored what the exchange gave it without a question: a new
    /// contact if it held nothing, the refreshed card if it held the peer.
    Saved(Holding),
    /// It held the peer, the peer held nothing, and its person agreed to a
    /// new connection: the contact now carries this exchange's key.
    Reconnected,
    /// The connection it had stays as it was. From a holder, its person
    /// declined or did not answer; from a phone that held nothing, it read
    /// the holder's `Kept` and saved nothing.
    Kept(Holding),
}

impl Ending {
    /// Whether the sender now holds a contact for the receiver under this
    /// exchange's key or the key it already had.
    pub fn saved(self) -> bool {
        !matches!(self, Ending::Kept(_))
    }
}

/// What the tag says: "I accepted you" (confirm) or how the exchange ended
/// here (done), each with what the sender holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagLabel {
    Confirm(Holding),
    Done(Ending),
}

/// The frame a tag was read from, which bounds the labels it may carry: a
/// confirm cannot be read as a done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    Confirm,
    Done,
}

impl TagKind {
    fn labels(self) -> &'static [TagLabel] {
        match self {
            TagKind::Confirm => &[
                TagLabel::Confirm(Holding::Peer),
                TagLabel::Confirm(Holding::Nothing),
            ],
            TagKind::Done => &[
                TagLabel::Done(Ending::Saved(Holding::Peer)),
                TagLabel::Done(Ending::Saved(Holding::Nothing)),
                TagLabel::Done(Ending::Reconnected),
                TagLabel::Done(Ending::Kept(Holding::Peer)),
                TagLabel::Done(Ending::Kept(Holding::Nothing)),
            ],
        }
    }
}

impl TagLabel {
    /// What the sender holds, where the label says it: `Reconnected` is
    /// only ever said by a holder.
    pub fn holding(self) -> Holding {
        match self {
            TagLabel::Confirm(holding)
            | TagLabel::Done(Ending::Saved(holding) | Ending::Kept(holding)) => holding,
            TagLabel::Done(Ending::Reconnected) => Holding::Peer,
        }
    }

    // Eight bytes each: every label is followed by fixed-width fields, so
    // equal lengths keep one label from being read as the start of another.
    fn bytes(self) -> &'static [u8; 8] {
        match self {
            TagLabel::Confirm(Holding::Peer) => b"CNF-HELD",
            TagLabel::Confirm(Holding::Nothing) => b"CNF-NONE",
            TagLabel::Done(Ending::Saved(Holding::Peer)) => b"DON-SV-H",
            TagLabel::Done(Ending::Saved(Holding::Nothing)) => b"DON-SV-N",
            TagLabel::Done(Ending::Reconnected) => b"DON-RECO",
            TagLabel::Done(Ending::Kept(Holding::Peer)) => b"DON-KP-H",
            TagLabel::Done(Ending::Kept(Holding::Nothing)) => b"DON-KP-N",
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

/// The label `tag` was made under, among those a frame of `kind` may
/// carry; `None` for a tag from another key, binding, payload or kind.
pub fn read(
    transport_key: &[u8; 32],
    kind: TagKind,
    tag: &[u8; 32],
    sender: &Side<'_>,
    receiver: &Side<'_>,
) -> Option<TagLabel> {
    kind.labels().iter().copied().fold(None, |found, label| {
        let expected = self::tag(transport_key, label, sender, receiver);
        if bool::from(tag.ct_eq(&expected)) {
            Some(label)
        } else {
            found
        }
    })
}

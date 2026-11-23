// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core's key schedule: the seed epoch, an epoch opened on a
//! refresh, the notice chain's step, and the addresses of refreshes and
//! openings. Every derivation is HKDF-SHA256 under its own label.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::{DhError, HKDF, X25519KeyPair};

const KEY_BYTES: usize = 32;
const TAG_BYTES: usize = 16;

/// A direction's name in every derivation: the sender's device public
/// key followed by the receiver's.
#[derive(Clone, PartialEq, Eq)]
pub struct Label([u8; 64]);

impl Label {
    pub fn new(sender_device_public: &[u8; 32], receiver_device_public: &[u8; 32]) -> Self {
        let mut label = [0u8; 64];
        let (sender, receiver) = label.split_at_mut(KEY_BYTES);
        sender.copy_from_slice(sender_device_public);
        receiver.copy_from_slice(receiver_device_public);
        Self(label)
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

type Key = Zeroizing<[u8; KEY_BYTES]>;

/// The keys of one epoch of one direction.
#[cfg_attr(feature = "testing", derive(Clone))]
pub struct Epoch {
    number: u32,
    id: [u8; TAG_BYTES],
    root: Key,
    notice_chain: Option<Key>,
    record: Key,
    control: Key,
    opening: Key,
    proof: Key,
}

impl Epoch {
    /// The number of the refresh the epoch was opened on; 0 for the seed.
    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn id(&self) -> [u8; TAG_BYTES] {
        self.id
    }

    pub fn root(&self) -> &[u8; KEY_BYTES] {
        &self.root
    }

    /// The notice chain's key at position 0, once. Whoever takes it
    /// steps it and erases what it stepped past; the epoch keeps no copy,
    /// or a later copy of the device could read the whole chain again.
    pub fn take_notice_chain(&mut self) -> Option<Zeroizing<[u8; KEY_BYTES]>> {
        self.notice_chain.take()
    }

    pub fn record_key(&self) -> &[u8; KEY_BYTES] {
        &self.record
    }

    pub fn control_key(&self) -> &[u8; KEY_BYTES] {
        &self.control
    }

    pub fn opening_key(&self) -> &[u8; KEY_BYTES] {
        &self.opening
    }

    pub fn proof_key(&self) -> &[u8; KEY_BYTES] {
        &self.proof
    }
}

/// An epoch as one device derived it, with the confirmation that the
/// other derived the same secret.
pub struct Opened {
    pub epoch: Epoch,
    pub confirmation: [u8; TAG_BYTES],
}

/// A 16-byte tag a reader recognises a record by, and the single-use key
/// the record is sealed under.
#[cfg_attr(feature = "testing", derive(Clone))]
pub struct Keyed {
    pub tag: [u8; TAG_BYTES],
    pub key: Key,
}

pub struct ChainStep {
    pub at: Keyed,
    pub next: Key,
}

fn info(domain: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut info = domain.to_vec();
    for part in parts {
        info.extend_from_slice(part);
    }
    info
}

fn key_at(okm: &[u8], index: usize) -> Key {
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    let chunk = okm.chunks_exact(KEY_BYTES).nth(index).unwrap_or_else(|| {
        unreachable!("the derivation's length covers every key it is split into")
    });
    key.copy_from_slice(chunk);
    key
}

fn tag_then_keys(okm: &[u8]) -> ([u8; TAG_BYTES], &[u8]) {
    let (tag_bytes, keys) = okm.split_at(TAG_BYTES);
    let mut tag = [0u8; TAG_BYTES];
    tag.copy_from_slice(tag_bytes);
    (tag, keys)
}

fn epoch_id(root: &[u8; KEY_BYTES]) -> [u8; TAG_BYTES] {
    *HKDF::derive_array::<TAG_BYTES>(Some(root), b"", b"Vauchi_Notice_Epoch_v7")
}

/// The epoch both devices start from, out of the seed handshake's
/// Diffie-Hellman result. Counts as an epoch the sender opened and the
/// receiver named.
pub fn seed_epoch(handshake_secret: &[u8; 32], label: &Label) -> Epoch {
    let seed = HKDF::derive_array::<KEY_BYTES>(
        None,
        handshake_secret,
        &info(b"Vauchi_Notice_Handshake_v7|", &[label.as_bytes()]),
    );
    let okm = HKDF::derive_array::<192>(
        None,
        seed.as_ref(),
        &info(b"Vauchi_Notice_Seed_v7|", &[label.as_bytes()]),
    );
    let root = key_at(okm.as_ref(), 0);
    Epoch {
        number: 0,
        id: epoch_id(&root),
        root,
        notice_chain: Some(key_at(okm.as_ref(), 1)),
        record: key_at(okm.as_ref(), 2),
        control: key_at(okm.as_ref(), 3),
        opening: key_at(okm.as_ref(), 4),
        proof: key_at(okm.as_ref(), 5),
    }
}

/// The three Diffie-Hellman results of an opening, as either side has them.
struct Agreed<'a> {
    ephemeral_with_refresh: &'a [u8; 32],
    sender_device_with_refresh: &'a [u8; 32],
    ephemeral_with_receiver_device: &'a [u8; 32],
}

fn opened(
    base: &Epoch,
    label: &Label,
    number: u32,
    ephemeral_public: &[u8; 32],
    refresh_public: &[u8; 32],
    agreed: Agreed<'_>,
) -> Opened {
    let number_bytes = number.to_be_bytes();
    let base_id = base.id();
    // The opening's whole context goes into what it derives: X25519 gives
    // one secret for several encodings of a public key, and the number
    // alone does not say which keys or which base an epoch came from.
    let transcript = |domain: &[u8]| {
        info(
            domain,
            &[
                label.as_bytes(),
                &number_bytes,
                ephemeral_public,
                refresh_public,
                &base_id,
            ],
        )
    };
    let mut secret = Zeroizing::new([0u8; 64]);
    let (first, second) = secret.split_at_mut(KEY_BYTES);
    first.copy_from_slice(agreed.ephemeral_with_refresh);
    second.copy_from_slice(agreed.sender_device_with_refresh);

    let confirmation = *HKDF::derive_array::<TAG_BYTES>(
        Some(base.root()),
        secret.as_ref(),
        &transcript(b"Vauchi_Notice_Confirm_v7|"),
    );
    let okm = HKDF::derive_array::<160>(
        Some(base.root()),
        secret.as_ref(),
        &transcript(b"Vauchi_Notice_Root_v7|"),
    );
    let root = key_at(okm.as_ref(), 0);
    // Salted with the new root: a copy of the receiver's device key, with
    // the openings it reads, must not give the proof key of later epochs.
    let proof = HKDF::derive_array::<KEY_BYTES>(
        Some(root.as_ref()),
        agreed.ephemeral_with_receiver_device,
        &transcript(b"Vauchi_Notice_Proof_v7|"),
    );
    Opened {
        epoch: Epoch {
            number,
            id: epoch_id(&root),
            root,
            notice_chain: Some(key_at(okm.as_ref(), 1)),
            record: key_at(okm.as_ref(), 2),
            control: key_at(okm.as_ref(), 3),
            opening: key_at(okm.as_ref(), 4),
            proof,
        },
        confirmation,
    }
}

/// The sender opens an epoch on refresh `number` with key
/// `refresh_public`, from the epoch the refresh was sealed in.
pub fn open_as_sender(
    base: &Epoch,
    label: &Label,
    number: u32,
    refresh_public: &[u8; 32],
    ephemeral: &X25519KeyPair,
    sender_device: &X25519KeyPair,
    receiver_device_public: &[u8; 32],
) -> Result<Opened, DhError> {
    Ok(opened(
        base,
        label,
        number,
        ephemeral.public_key(),
        refresh_public,
        Agreed {
            ephemeral_with_refresh: &*ephemeral.diffie_hellman(refresh_public)?,
            sender_device_with_refresh: &*sender_device.diffie_hellman(refresh_public)?,
            ephemeral_with_receiver_device: &*ephemeral.diffie_hellman(receiver_device_public)?,
        },
    ))
}

/// The receiver derives the epoch the sender opened, from the opening's
/// ephemeral public key.
pub fn open_as_receiver(
    base: &Epoch,
    label: &Label,
    number: u32,
    refresh: &X25519KeyPair,
    ephemeral_public: &[u8; 32],
    sender_device_public: &[u8; 32],
    receiver_device: &X25519KeyPair,
) -> Result<Opened, DhError> {
    Ok(opened(
        base,
        label,
        number,
        ephemeral_public,
        refresh.public_key(),
        Agreed {
            ephemeral_with_refresh: &*refresh.diffie_hellman(ephemeral_public)?,
            sender_device_with_refresh: &*refresh.diffie_hellman(sender_device_public)?,
            ephemeral_with_receiver_device: &*receiver_device.diffie_hellman(ephemeral_public)?,
        },
    ))
}

/// What an opening names the refresh key by.
pub fn key_id(refresh_public: &[u8; 32]) -> [u8; 8] {
    let digest = Sha256::digest(refresh_public);
    let mut id = [0u8; 8];
    for (to, from) in id.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    id
}

/// One step of a notice chain: the position's tag and message key, and
/// the next chain key.
pub fn chain_step(chain_key: &[u8; 32]) -> ChainStep {
    let okm = HKDF::derive_array::<80>(None, chain_key, b"Vauchi_Notice_Chain_v7");
    let (tag, keys) = tag_then_keys(okm.as_ref());
    ChainStep {
        at: Keyed {
            tag,
            key: key_at(keys, 0),
        },
        next: key_at(keys, 1),
    }
}

fn address(key: &[u8; 32], info: &[u8]) -> Keyed {
    let okm = HKDF::derive_array::<48>(Some(key), b"", info);
    let (tag, keys) = tag_then_keys(okm.as_ref());
    Keyed {
        tag,
        key: key_at(keys, 0),
    }
}

/// Where the receiver seals a refresh in an epoch: by its week and an
/// attempt from 0 to 3.
pub fn refresh_address(control_key: &[u8; 32], week: u32, attempt: u8) -> Keyed {
    address(
        control_key,
        &info(
            b"Vauchi_Notice_Refresh_v7|",
            &[&week.to_be_bytes(), &[attempt]],
        ),
    )
}

/// Where the sender seals the opening that answers refresh `number`,
/// in the epoch that refresh was sealed in.
pub fn opening_address(opening_key: &[u8; 32], number: u32, attempt: u8) -> Keyed {
    address(
        opening_key,
        &info(
            b"Vauchi_Notice_Open_v7|",
            &[&number.to_be_bytes(), &[attempt]],
        ),
    )
}

/// What binds a refresh to the epoch it names without sending the name.
pub fn proof_tag(
    proof_key: &[u8; 32],
    number: u32,
    refresh_public: &[u8; 32],
    named_epoch_id: &[u8; TAG_BYTES],
) -> [u8; TAG_BYTES] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(proof_key)
        .unwrap_or_else(|_| unreachable!("HMAC-SHA256 takes a key of any length"));
    mac.update(b"refresh");
    mac.update(&number.to_be_bytes());
    mac.update(refresh_public);
    mac.update(named_epoch_id);
    let digest = mac.finalize().into_bytes();
    let mut tag = [0u8; TAG_BYTES];
    for (to, from) in tag.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    tag
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content records: a card body in 576-byte records under an update key.
//!
//! A record is found by a 16-byte ID derived from the update key, the
//! publication's week, the record's index and its transmission, so no ID
//! is ever written twice and the gateway links neither publications nor
//! resends. The notice's commitment is over the body, so chunks from
//! different publications of one key complete one body.

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::crypto::HKDF;

pub const ID_BYTES: usize = 16;
pub const NONCE_BYTES: usize = 24;
pub const CHUNK_BYTES: usize = 512;
const HEADER_BYTES: usize = 8;
const AEAD_TAG_BYTES: usize = 16;
pub const RECORD_BYTES: usize =
    ID_BYTES + NONCE_BYTES + HEADER_BYTES + CHUNK_BYTES + AEAD_TAG_BYTES;
pub const MAX_RECORDS: u16 = 8;
pub const MAX_BODY_BYTES: usize = CHUNK_BYTES * MAX_RECORDS as usize;
/// How many times one record of one publication may be written.
pub const TRANSMISSIONS: u8 = 4;
pub const COMMITMENT_BYTES: usize = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentError {
    /// A body is 1 to 4,096 bytes.
    BodySize,
    /// A record's index lies inside its body.
    Index,
    /// A record is written at most four times in a publication.
    Transmission,
    /// A chunk does not agree with the chunks already held.
    Disagrees,
}

/// How many records a body of `length` bytes takes.
pub fn record_count(length: usize) -> Result<u16, ContentError> {
    if length == 0 || length > MAX_BODY_BYTES {
        return Err(ContentError::BodySize);
    }
    u16::try_from(length.div_ceil(CHUNK_BYTES)).map_err(|_| ContentError::BodySize)
}

fn record_key(update_key: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    HKDF::derive_array::<32>(None, update_key, b"Vauchi_Update_Record_v4")
}

/// What a notice binds a body by.
pub fn commitment(update_key: &[u8; 32], body: &[u8]) -> [u8; COMMITMENT_BYTES] {
    let commit_key = HKDF::derive_array::<32>(None, update_key, b"Vauchi_Update_Record_v4|commit");
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(commit_key.as_ref())
        .unwrap_or_else(|_| unreachable!("HMAC-SHA256 takes a key of any length"));
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let mut commitment = [0u8; COMMITMENT_BYTES];
    for (to, from) in commitment.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    commitment
}

/// Where record `index` of the publication of `week` travels on its
/// `transmission`-th writing.
pub fn record_id(update_key: &[u8; 32], week: u32, index: u16, transmission: u8) -> [u8; ID_BYTES] {
    let mut info = b"Vauchi_Update_Record_v4|id".to_vec();
    info.extend_from_slice(&week.to_be_bytes());
    info.extend_from_slice(&index.to_be_bytes());
    info.push(transmission);
    *HKDF::derive_array::<ID_BYTES>(None, update_key, &info)
}

/// Seals record `index` of `body`: `ID ‖ nonce ‖ AEAD(index ‖ count ‖
/// length ‖ chunk)`. The nonce must be fresh random bytes.
pub fn seal_record(
    update_key: &[u8; 32],
    body: &[u8],
    week: u32,
    index: u16,
    transmission: u8,
    nonce: &[u8; NONCE_BYTES],
) -> Result<[u8; RECORD_BYTES], ContentError> {
    let count = record_count(body.len())?;
    if index >= count {
        return Err(ContentError::Index);
    }
    if transmission >= TRANSMISSIONS {
        return Err(ContentError::Transmission);
    }
    let length = u32::try_from(body.len()).map_err(|_| ContentError::BodySize)?;
    let chunk = body
        .chunks(CHUNK_BYTES)
        .nth(usize::from(index))
        .ok_or(ContentError::Index)?;

    let mut plaintext = Zeroizing::new(Vec::with_capacity(HEADER_BYTES + CHUNK_BYTES));
    plaintext.extend_from_slice(&index.to_be_bytes());
    plaintext.extend_from_slice(&count.to_be_bytes());
    plaintext.extend_from_slice(&length.to_be_bytes());
    plaintext.extend_from_slice(chunk);
    plaintext.resize(HEADER_BYTES + CHUNK_BYTES, 0);

    let id = record_id(update_key, week, index, transmission);
    let key = record_key(update_key);
    let sealed = XChaCha20Poly1305::new(key.as_ref().into())
        .encrypt(
            chacha20poly1305::XNonce::from_slice(nonce),
            Payload {
                msg: plaintext.as_slice(),
                aad: &id,
            },
        )
        .unwrap_or_else(|_| unreachable!("a 520-byte plaintext is within the AEAD's limits"));

    let mut record = [0u8; RECORD_BYTES];
    let parts = id.iter().chain(nonce.iter()).chain(sealed.iter());
    for (to, from) in record.iter_mut().zip(parts) {
        *to = *from;
    }
    Ok(record)
}

/// One record's part of a body.
#[derive(Clone)]
pub struct Chunk {
    pub index: u16,
    pub count: u16,
    pub length: u32,
    pub bytes: Zeroizing<[u8; CHUNK_BYTES]>,
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (field, rest) = self.rest.split_first_chunk::<N>()?;
        self.rest = rest;
        Some(*field)
    }
}

/// Opens a record expected as record `index` of the publication of
/// `week`, on its `transmission`-th writing. `None` for a record that is
/// not that, or whose count or length cannot be a body's.
pub fn open_record(
    update_key: &[u8; 32],
    week: u32,
    index: u16,
    transmission: u8,
    record: &[u8; RECORD_BYTES],
) -> Option<Chunk> {
    let expected_id = record_id(update_key, week, index, transmission);
    let id = &expected_id;
    let mut reader = Reader { rest: record };
    let carried_id = reader.take::<ID_BYTES>()?;
    if !bool::from(carried_id.ct_eq(id)) {
        return None;
    }
    let nonce = reader.take::<NONCE_BYTES>()?;
    let key = record_key(update_key);
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new(key.as_ref().into())
            .decrypt(
                chacha20poly1305::XNonce::from_slice(&nonce),
                Payload {
                    msg: reader.rest,
                    aad: id,
                },
            )
            .ok()?,
    );

    let mut fields = Reader {
        rest: plaintext.as_slice(),
    };
    let carried_index = u16::from_be_bytes(fields.take::<2>()?);
    if carried_index != index {
        return None;
    }
    let count = u16::from_be_bytes(fields.take::<2>()?);
    let length = u32::from_be_bytes(fields.take::<4>()?);
    let bytes = Zeroizing::new(fields.take::<CHUNK_BYTES>()?);
    if !fields.rest.is_empty() {
        return None;
    }

    let expected_count = record_count(usize::try_from(length).ok()?).ok()?;
    if count != expected_count || index >= count {
        return None;
    }
    let used = if index.saturating_add(1) == count {
        usize::try_from(length)
            .ok()?
            .saturating_sub(CHUNK_BYTES.saturating_mul(usize::from(index)))
    } else {
        CHUNK_BYTES
    };
    if !bytes.iter().skip(used).all(|byte| *byte == 0) {
        return None;
    }
    Some(Chunk {
        index,
        count,
        length,
        bytes,
    })
}

/// The chunks of one update as they arrive, from any publication.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Assembly {
    commitment: [u8; COMMITMENT_BYTES],
    shape: Option<(u16, u32)>,
    #[serde(with = "crate::notice::saved::chunks")]
    chunks: Vec<Option<Zeroizing<[u8; CHUNK_BYTES]>>>,
}

impl Assembly {
    pub fn new(commitment: [u8; COMMITMENT_BYTES]) -> Self {
        Self {
            commitment,
            shape: None,
            chunks: Vec::new(),
        }
    }

    /// Keeps a chunk. One that tells another count or length than the
    /// chunks held, or differs from the chunk held at its index, is
    /// refused and changes nothing.
    pub fn add(&mut self, chunk: Chunk) -> Result<(), ContentError> {
        match self.shape {
            None => {
                self.shape = Some((chunk.count, chunk.length));
                self.chunks = (0..chunk.count).map(|_| None).collect();
            }
            Some(shape) if shape != (chunk.count, chunk.length) => {
                return Err(ContentError::Disagrees);
            }
            Some(_) => {}
        }
        let slot = self
            .chunks
            .get_mut(usize::from(chunk.index))
            .ok_or(ContentError::Index)?;
        match slot {
            Some(held) if !bool::from(held.ct_eq(chunk.bytes.as_ref())) => {
                Err(ContentError::Disagrees)
            }
            Some(_) => Ok(()),
            None => {
                *slot = Some(chunk.bytes);
                Ok(())
            }
        }
    }

    /// The indices not yet held; every index while no chunk told the
    /// count.
    pub fn missing(&self) -> Vec<u16> {
        match self.shape {
            None => (0..MAX_RECORDS).collect(),
            Some((count, _)) => (0..count)
                .filter(|index| matches!(self.chunks.get(usize::from(*index)), Some(None)))
                .collect(),
        }
    }

    /// The body, once every chunk is held and it matches the commitment
    /// the notice carried.
    pub fn complete(&self, update_key: &[u8; 32]) -> Option<Zeroizing<Vec<u8>>> {
        let (_, length) = self.shape?;
        let mut body = Zeroizing::new(Vec::new());
        for chunk in &self.chunks {
            body.extend_from_slice(chunk.as_ref()?.as_ref());
        }
        body.truncate(usize::try_from(length).ok()?);
        let expected = commitment(update_key, &body);
        bool::from(expected.ct_eq(&self.commitment)).then_some(body)
    }
}

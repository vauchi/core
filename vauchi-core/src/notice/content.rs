// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content records: a card body in 576-byte records under an update key.
//!
//! A record is found by a 16-byte ID derived from the update key, the
//! publication's week, the record's index and its transmission, so no ID
//! is ever written twice and the gateway links neither publications nor
//! resends. Record 0 begins with a 16-byte hash of every later chunk and
//! the notice's commitment binds record 0, so each record is checked on
//! its own: a group member, who holds the update key too, cannot make a
//! forged record stand in for an honest one (owner, 2026-11-23, gate
//! review 7). Chunks from different publications of one key complete one
//! body.

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
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
/// A later chunk's hash as record 0 carries it.
pub const CHUNK_HASH_BYTES: usize = 16;
pub const MAX_BODY_BYTES: usize =
    CHUNK_BYTES * MAX_RECORDS as usize - CHUNK_HASH_BYTES * (MAX_RECORDS as usize - 1);
/// How many times one record of one publication may be written.
pub const TRANSMISSIONS: u8 = 4;
pub const COMMITMENT_BYTES: usize = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentError {
    /// A body is 1 to 3,984 bytes.
    BodySize,
    /// A record's index lies inside its body.
    Index,
    /// A record is written at most four times in a publication.
    Transmission,
    /// A chunk does not agree with the chunks already held.
    Disagrees,
    /// A later chunk waits for record 0, which carries its hash.
    NeedsFirstRecord,
}

/// How many records a body of `length` bytes takes: record 0 also holds
/// the hash of every later chunk.
pub fn record_count(length: usize) -> Result<u16, ContentError> {
    if length == 0 || length > MAX_BODY_BYTES {
        return Err(ContentError::BodySize);
    }
    let mut count = 1usize;
    while CHUNK_BYTES * count < length + CHUNK_HASH_BYTES * (count - 1) {
        count += 1;
    }
    u16::try_from(count).map_err(|_| ContentError::BodySize)
}

fn manifest_bytes(count: u16) -> usize {
    CHUNK_HASH_BYTES * usize::from(count.saturating_sub(1))
}

fn record_key(update_key: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    HKDF::derive_array::<32>(None, update_key, b"Vauchi_Update_Record_v5")
}

fn chunk_hash(index: u16, chunk: &[u8; CHUNK_BYTES]) -> [u8; CHUNK_HASH_BYTES] {
    let digest = Sha256::new()
        .chain_update(b"Vauchi_Update_Chunk_v5")
        .chain_update(index.to_be_bytes())
        .chain_update(chunk)
        .finalize();
    let mut hash = [0u8; CHUNK_HASH_BYTES];
    hash.copy_from_slice(&digest[..CHUNK_HASH_BYTES]);
    hash
}

/// Each record's 512 bytes: the later chunks' hashes, then the body,
/// zero-padded.
fn chunks_of(body: &[u8]) -> Result<Vec<Zeroizing<[u8; CHUNK_BYTES]>>, ContentError> {
    let count = record_count(body.len())?;
    let mut payload = Zeroizing::new(vec![0u8; manifest_bytes(count)]);
    payload.extend_from_slice(body);
    payload.resize(CHUNK_BYTES * usize::from(count), 0);
    let mut chunks: Vec<Zeroizing<[u8; CHUNK_BYTES]>> = payload
        .chunks(CHUNK_BYTES)
        .map(|part| {
            let mut chunk = Zeroizing::new([0u8; CHUNK_BYTES]);
            chunk.copy_from_slice(part);
            chunk
        })
        .collect();
    for index in 1..count {
        let hash = chunk_hash(index, &chunks[usize::from(index)]);
        let at = CHUNK_HASH_BYTES * usize::from(index - 1);
        chunks[0][at..at + CHUNK_HASH_BYTES].copy_from_slice(&hash);
    }
    Ok(chunks)
}

fn first_record_mac(
    update_key: &[u8; 32],
    count: u16,
    length: u32,
    first: &[u8; CHUNK_BYTES],
) -> [u8; COMMITMENT_BYTES] {
    let commit_key = HKDF::derive_array::<32>(None, update_key, b"Vauchi_Update_Record_v5|commit");
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(commit_key.as_ref())
        .unwrap_or_else(|_| unreachable!("HMAC-SHA256 takes a key of any length"));
    mac.update(&count.to_be_bytes());
    mac.update(&length.to_be_bytes());
    mac.update(first);
    let digest = mac.finalize().into_bytes();
    let mut commitment = [0u8; COMMITMENT_BYTES];
    for (to, from) in commitment.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    commitment
}

/// What a notice binds a body by: record 0, and through its hashes every
/// later chunk.
///
/// # Panics
/// If `body` is not 1 to 3,984 bytes.
pub fn commitment(update_key: &[u8; 32], body: &[u8]) -> [u8; COMMITMENT_BYTES] {
    let chunks = chunks_of(body).expect("a body of 1 to 3,984 bytes");
    let count = u16::try_from(chunks.len()).unwrap_or(u16::MAX);
    let length = u32::try_from(body.len()).unwrap_or(u32::MAX);
    first_record_mac(update_key, count, length, &chunks[0])
}

/// Where record `index` of the publication of `week` travels on its
/// `transmission`-th writing.
pub fn record_id(update_key: &[u8; 32], week: u32, index: u16, transmission: u8) -> [u8; ID_BYTES] {
    let mut info = b"Vauchi_Update_Record_v5|id".to_vec();
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
    let chunks = chunks_of(body)?;
    let chunk = chunks.get(usize::from(index)).ok_or(ContentError::Index)?;

    let mut plaintext = Zeroizing::new(Vec::with_capacity(HEADER_BYTES + CHUNK_BYTES));
    plaintext.extend_from_slice(&index.to_be_bytes());
    plaintext.extend_from_slice(&count.to_be_bytes());
    plaintext.extend_from_slice(&length.to_be_bytes());
    plaintext.extend_from_slice(chunk.as_ref());

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
    let payload = usize::try_from(length)
        .ok()?
        .saturating_add(manifest_bytes(count));
    let used = payload
        .saturating_sub(CHUNK_BYTES.saturating_mul(usize::from(index)))
        .min(CHUNK_BYTES);
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

/// The chunks of one update as they arrive, from any publication. Record
/// 0 is held only once it matches the notice's commitment, and a later
/// chunk only once it matches the hash record 0 carries for it: a forged
/// record changes nothing.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Assembly {
    commitment: [u8; COMMITMENT_BYTES],
    shape: Option<(u16, u32)>,
    hashes: Vec<[u8; CHUNK_HASH_BYTES]>,
    #[serde(with = "crate::notice::saved::chunks")]
    chunks: Vec<Option<Zeroizing<[u8; CHUNK_BYTES]>>>,
}

impl Assembly {
    pub fn new(commitment: [u8; COMMITMENT_BYTES]) -> Self {
        Self {
            commitment,
            shape: None,
            hashes: Vec::new(),
            chunks: Vec::new(),
        }
    }

    /// Keeps a chunk that checks out; anything else is refused and changes
    /// nothing.
    pub fn add(&mut self, update_key: &[u8; 32], chunk: Chunk) -> Result<(), ContentError> {
        if chunk.index == 0 {
            return self.add_first(update_key, chunk);
        }
        let Some((count, length)) = self.shape else {
            return Err(ContentError::NeedsFirstRecord);
        };
        if (chunk.count, chunk.length) != (count, length) || chunk.index >= count {
            return Err(ContentError::Disagrees);
        }
        let expected = self
            .hashes
            .get(usize::from(chunk.index - 1))
            .ok_or(ContentError::Index)?;
        if !bool::from(chunk_hash(chunk.index, &chunk.bytes).ct_eq(expected)) {
            return Err(ContentError::Disagrees);
        }
        if let Some(slot) = self.chunks.get_mut(usize::from(chunk.index)) {
            slot.get_or_insert(chunk.bytes);
        }
        Ok(())
    }

    fn add_first(&mut self, update_key: &[u8; 32], chunk: Chunk) -> Result<(), ContentError> {
        let expected = first_record_mac(update_key, chunk.count, chunk.length, &chunk.bytes);
        if !bool::from(expected.ct_eq(&self.commitment)) {
            return Err(ContentError::Disagrees);
        }
        if self.shape.is_none() {
            self.shape = Some((chunk.count, chunk.length));
            self.hashes = chunk.bytes[..manifest_bytes(chunk.count)]
                .chunks(CHUNK_HASH_BYTES)
                .map(|hash| {
                    let mut held = [0u8; CHUNK_HASH_BYTES];
                    held.copy_from_slice(hash);
                    held
                })
                .collect();
            self.chunks = (0..chunk.count).map(|_| None).collect();
            self.chunks[0] = Some(chunk.bytes);
        }
        Ok(())
    }

    /// The indices not yet held; every index while record 0 is missing.
    pub fn missing(&self) -> Vec<u16> {
        match self.shape {
            None => (0..MAX_RECORDS).collect(),
            Some((count, _)) => (0..count)
                .filter(|index| matches!(self.chunks.get(usize::from(*index)), Some(None)))
                .collect(),
        }
    }

    /// The body, once every chunk is held; each was checked when it came.
    pub fn complete(&self) -> Option<Zeroizing<Vec<u8>>> {
        let (count, length) = self.shape?;
        let mut payload = Zeroizing::new(Vec::new());
        for chunk in &self.chunks {
            payload.extend_from_slice(chunk.as_ref()?.as_ref());
        }
        let start = manifest_bytes(count);
        let end = start.checked_add(usize::try_from(length).ok()?)?;
        Some(Zeroizing::new(payload.get(start..end)?.to_vec()))
    }
}

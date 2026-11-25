// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core's saved state (protocol section 5; vauchi/private#1281):
//! one kind byte, one format version, then the state in postcard. Storage
//! encrypts it at rest (ADR-015); this module only encodes it.
//!
//! The bytes hold secret keys, so they are returned zeroized on drop. A
//! load refuses anything that is not exactly one state of the kind asked
//! for, with one error that says nothing about why.

use serde::Serialize;
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

use crate::crypto::X25519KeyPair;

/// 2: openings, publishers and group entries gained fields (gate reviews
/// 10 and 11); 3: openings keep when they were queued (gate review 12);
/// 4: one opening a direction and alternating control turns (owner,
/// 2026-11-23); 5: no highest refresh number kept (gate review 14).
/// State saved under an earlier version is refused, not misread.
const FORMAT_VERSION: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedError;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Receiver = 1,
    Sender = 2,
    Publisher = 3,
    Pending = 4,
    Session = 5,
    Groups = 6,
}

/// A state that cannot be encoded is an error, never an empty save that a
/// later load refuses (gate review 9, Claude).
pub(crate) fn save<T: Serialize>(kind: Kind, state: &T) -> Result<Zeroizing<Vec<u8>>, SavedError> {
    let mut bytes = Zeroizing::new(vec![kind as u8, FORMAT_VERSION]);
    let encoded = Zeroizing::new(postcard::to_allocvec(state).map_err(|_| SavedError)?);
    bytes.extend_from_slice(&encoded);
    Ok(bytes)
}

pub(crate) fn load<T: DeserializeOwned>(kind: Kind, bytes: &[u8]) -> Result<T, SavedError> {
    let [found_kind, version, rest @ ..] = bytes else {
        return Err(SavedError);
    };
    if *found_kind != kind as u8 || *version != FORMAT_VERSION {
        return Err(SavedError);
    }
    let (state, trailing) = postcard::take_from_bytes(rest).map_err(|_| SavedError)?;
    if !trailing.is_empty() {
        return Err(SavedError);
    }
    Ok(state)
}

/// A key pair saved as its secret scalar; the public key is recomputed.
pub(crate) mod key_pair {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(key: &X25519KeyPair, to: S) -> Result<S::Ok, S::Error> {
        key.secret_bytes().serialize(to)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<X25519KeyPair, D::Error> {
        let secret = Zeroizing::new(<[u8; 32]>::deserialize(from)?);
        Ok(X25519KeyPair::from_bytes(*secret))
    }
}

pub(crate) mod optional_key_pair {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(
        key: &Option<X25519KeyPair>,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        key.as_ref().map(X25519KeyPair::secret_bytes).serialize(to)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<Option<X25519KeyPair>, D::Error> {
        let secret = Option::<[u8; 32]>::deserialize(from)?.map(Zeroizing::new);
        Ok(secret.map(|secret| X25519KeyPair::from_bytes(*secret)))
    }
}

/// Fixed-size secret arrays longer than serde's 32, saved as bytes and
/// refused unless the length is exact.
pub(crate) mod secret_array {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(crate) fn serialize<S: Serializer, const N: usize>(
        value: &Zeroizing<[u8; N]>,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        to.serialize_bytes(value.as_ref())
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        from: D,
    ) -> Result<Zeroizing<[u8; N]>, D::Error> {
        let bytes = Zeroizing::new(<Vec<u8>>::deserialize(from)?);
        let mut value = Zeroizing::new([0u8; N]);
        if bytes.len() != N {
            return Err(D::Error::custom("wrong length"));
        }
        value.copy_from_slice(&bytes);
        Ok(value)
    }
}

/// An assembly's chunks: each held one saved with `secret_array`.
pub(crate) mod chunks {
    use super::*;
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::notice::content::CHUNK_BYTES;

    type HeldChunks = Vec<Option<Zeroizing<[u8; CHUNK_BYTES]>>>;

    #[derive(Serialize, Deserialize)]
    struct Held(#[serde(with = "secret_array")] Zeroizing<[u8; CHUNK_BYTES]>);

    pub(crate) fn serialize<S: Serializer>(
        chunks: &[Option<Zeroizing<[u8; CHUNK_BYTES]>>],
        to: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = to.serialize_seq(Some(chunks.len()))?;
        for chunk in chunks {
            seq.serialize_element(&chunk.as_ref().map(|bytes| Held(bytes.clone())))?;
        }
        seq.end()
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<HeldChunks, D::Error> {
        let held = Vec::<Option<Held>>::deserialize(from)?;
        Ok(held
            .into_iter()
            .map(|chunk| chunk.map(|Held(bytes)| bytes))
            .collect())
    }
}

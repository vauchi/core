// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a notice says: one of three records in 64 bytes.

use zeroize::Zeroizing;

use super::seal::PLAINTEXT_BYTES;

const KIND_UPDATE_KEY: u8 = 0x00;
const KIND_REFRESH: u8 = 0x01;
const KIND_OPENING: u8 = 0x02;

/// A plaintext that is no record: an unknown kind, or nonzero bytes
/// where a record has none. A reader ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

#[derive(Clone)]
pub enum Record {
    /// The key of one card update and the commitment to its body.
    UpdateKey {
        update_key: Zeroizing<[u8; 32]>,
        commitment: [u8; 31],
    },
    /// The receiver's next ratchet key, with the proof that binds it to
    /// the epoch the receiver is in.
    Refresh {
        refresh_public: [u8; 32],
        number: u32,
        proof_tag: [u8; 16],
    },
    /// The sender's answer to a refresh: the epoch it opened on it.
    Opening {
        ephemeral_public: [u8; 32],
        number: u32,
        key_id: [u8; 8],
        confirmation: [u8; 16],
    },
}

/// Compares update keys in constant time; the other fields are public.
impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        match (self, other) {
            (
                Record::UpdateKey {
                    update_key: a,
                    commitment: ca,
                },
                Record::UpdateKey {
                    update_key: b,
                    commitment: cb,
                },
            ) => bool::from(a.as_ref().ct_eq(b.as_ref())) && ca == cb,
            (
                Record::Refresh {
                    refresh_public: a,
                    number: na,
                    proof_tag: pa,
                },
                Record::Refresh {
                    refresh_public: b,
                    number: nb,
                    proof_tag: pb,
                },
            ) => a == b && na == nb && pa == pb,
            (
                Record::Opening {
                    ephemeral_public: a,
                    number: na,
                    key_id: ka,
                    confirmation: fa,
                },
                Record::Opening {
                    ephemeral_public: b,
                    number: nb,
                    key_id: kb,
                    confirmation: fb,
                },
            ) => a == b && na == nb && ka == kb && fa == fb,
            _ => false,
        }
    }
}

impl Eq for Record {}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Record::UpdateKey { .. } => f.write_str("UpdateKey(..)"),
            Record::Refresh { number, .. } => write!(f, "Refresh({number})"),
            Record::Opening { number, .. } => write!(f, "Opening({number})"),
        }
    }
}

struct Writer {
    bytes: Zeroizing<[u8; PLAINTEXT_BYTES]>,
    written: usize,
}

impl Writer {
    fn new(kind: u8) -> Self {
        let mut writer = Self {
            bytes: Zeroizing::new([0u8; PLAINTEXT_BYTES]),
            written: 0,
        };
        writer.put(&[kind]);
        writer
    }

    fn put(&mut self, field: &[u8]) {
        let end = self.written.saturating_add(field.len());
        if let Some(target) = self.bytes.get_mut(self.written..end) {
            target.copy_from_slice(field);
        }
        self.written = end;
    }
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Malformed> {
        let (field, rest) = self.rest.split_first_chunk::<N>().ok_or(Malformed)?;
        self.rest = rest;
        Ok(*field)
    }

    /// A secret field, copied straight into memory that is wiped on drop.
    fn take_secret(&mut self) -> Result<Zeroizing<[u8; 32]>, Malformed> {
        let (field, rest) = self.rest.split_first_chunk::<32>().ok_or(Malformed)?;
        self.rest = rest;
        let mut secret = Zeroizing::new([0u8; 32]);
        secret.copy_from_slice(field);
        Ok(secret)
    }

    fn end_in_zeros(self) -> Result<(), Malformed> {
        if self.rest.iter().all(|byte| *byte == 0) {
            Ok(())
        } else {
            Err(Malformed)
        }
    }
}

impl Record {
    pub fn to_plaintext(&self) -> Zeroizing<[u8; PLAINTEXT_BYTES]> {
        match self {
            Record::UpdateKey {
                update_key,
                commitment,
            } => {
                let mut writer = Writer::new(KIND_UPDATE_KEY);
                writer.put(update_key.as_ref());
                writer.put(commitment);
                writer.bytes
            }
            Record::Refresh {
                refresh_public,
                number,
                proof_tag,
            } => {
                let mut writer = Writer::new(KIND_REFRESH);
                writer.put(refresh_public);
                writer.put(&number.to_be_bytes());
                writer.put(proof_tag);
                writer.bytes
            }
            Record::Opening {
                ephemeral_public,
                number,
                key_id,
                confirmation,
            } => {
                let mut writer = Writer::new(KIND_OPENING);
                writer.put(ephemeral_public);
                writer.put(&number.to_be_bytes());
                writer.put(key_id);
                writer.put(confirmation);
                writer.bytes
            }
        }
    }

    pub fn from_plaintext(plaintext: &[u8; PLAINTEXT_BYTES]) -> Result<Self, Malformed> {
        let mut reader = Reader { rest: plaintext };
        let [kind] = reader.take::<1>()?;
        match kind {
            KIND_UPDATE_KEY => {
                let update_key = reader.take_secret()?;
                let commitment = reader.take::<31>()?;
                reader.end_in_zeros()?;
                Ok(Record::UpdateKey {
                    update_key,
                    commitment,
                })
            }
            KIND_REFRESH => {
                let refresh_public = reader.take::<32>()?;
                let number = u32::from_be_bytes(reader.take::<4>()?);
                let proof_tag = reader.take::<16>()?;
                reader.end_in_zeros()?;
                Ok(Record::Refresh {
                    refresh_public,
                    number,
                    proof_tag,
                })
            }
            KIND_OPENING => {
                let ephemeral_public = reader.take::<32>()?;
                let number = u32::from_be_bytes(reader.take::<4>()?);
                let key_id = reader.take::<8>()?;
                let confirmation = reader.take::<16>()?;
                reader.end_in_zeros()?;
                Ok(Record::Opening {
                    ephemeral_public,
                    number,
                    key_id,
                    confirmation,
                })
            }
            _ => Err(Malformed),
        }
    }
}

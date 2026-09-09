// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ISO 7816-4 APDU framing for the Vauchi NFC exchange.
//!
//! Core owns the wire format so shells forward raw bytes (ADR-066):
//! the reader transmits [`frame_command`] / [`frame_exchange`] output
//! verbatim and hands every response to [`decode_response`]; the HCE
//! responder hands every command APDU to [`decode_command`] and returns
//! [`frame_response`] / [`status_response`] bytes verbatim.
//!
//! Wire contract (frozen by `tests/it/nfc_apdu_codec_tests.rs`):
//!
//! ```text
//! SELECT    00 A4 04 00 07 F0 56 41 55 43 48 49
//! EXCHANGE  CLA E0 00 00 Lc <data>      CLA = 10 while more chunks follow, 00 on the last
//! response  <data> SW1 SW2              SW 90 00 = success
//! ```
//!
//! Every reader in the field (iOS, Android, linux-gtk) already speaks
//! this; the constants below are the seven-byte AID the shells register
//! with their OS, not the eight-byte one the earlier core-only helper
//! carried.

pub use super::nfc_apdu_chaining::MAX_APDU_DATA;
use super::nfc_apdu_chaining::split_into_chain;

/// Vauchi NFC Application Identifier: F0 "VAUCHI".
pub const AID: &[u8] = &[0xF0, 0x56, 0x41, 0x55, 0x43, 0x48, 0x49];

/// ISO 7816 status word indicating successful command execution (SW1=90, SW2=00).
pub const SW_SUCCESS: [u8; 2] = [0x90, 0x00];
/// ISO 7816 status word indicating the requested AID was not found on the card (SW1=6A, SW2=82).
pub const SW_AID_NOT_FOUND: [u8; 2] = [0x6A, 0x82];
/// ISO 7816 status word indicating conditions of use not satisfied (SW1=69, SW2=85).
pub const SW_CONDITIONS_NOT_SATISFIED: [u8; 2] = [0x69, 0x85];
/// ISO 7816 status word indicating the command data was rejected (SW1=6A, SW2=80).
pub const SW_WRONG_DATA: [u8; 2] = [0x6A, 0x80];

/// Ceiling on the data carried by one command chain or one response
/// (DC-03). The largest legitimate frame is a 174-byte key ack plus one
/// encrypted card; anything an order of magnitude beyond that is a
/// hostile peer trying to grow the reassembly buffer.
pub const MAX_EXCHANGE_PAYLOAD: usize = 4096;

pub const INS_SELECT: u8 = 0xA4;
pub const INS_EXCHANGE_DATA: u8 = 0xE0;
pub const INS_CARD_EXCHANGE: u8 = 0xE2;

const CLA_CHAINING_BIT: u8 = 0x10;
const P1_SELECT_BY_NAME: u8 = 0x04;
const HEADER_LEN: usize = 4;
const SHORT_LC_LEN: usize = 1;
const EXTENDED_LC_LEN: usize = 3;

/// Why a frame could not be produced or understood. Carries lengths and
/// status words only — never payload bytes (DC-05).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApduError {
    #[error("NFC response shorter than a status word")]
    Truncated,
    #[error("NFC payload of {len} bytes exceeds the {max}-byte ceiling")]
    Oversized { len: usize, max: usize },
    #[error("The other device has no Vauchi app selected")]
    AidNotFound,
    #[error("The other device is not in an exchange")]
    ConditionsNotSatisfied,
    #[error("The other device rejected the payload")]
    WrongData,
    #[error("Unexpected NFC status word {0:02X}{1:02X}")]
    UnexpectedStatus(u8, u8),
    #[error("Malformed NFC command")]
    MalformedCommand,
    #[error("NFC command chain broken")]
    ChainBroken,
}

/// A decoded command APDU as seen by the HCE responder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApduCommand {
    /// `SELECT` for our AID — answer with [`SW_SUCCESS`], no state change.
    Select,
    /// One `EXCHANGE` chunk; `more` is the CLA chaining bit.
    Exchange { data: Vec<u8>, more: bool },
    /// Any other instruction.
    Unknown { ins: u8 },
}

/// Builds a SELECT APDU command for the Vauchi AID.
pub fn build_select() -> Vec<u8> {
    let mut cmd = Vec::with_capacity(HEADER_LEN + SHORT_LC_LEN + AID.len());
    cmd.push(0x00);
    cmd.push(INS_SELECT);
    cmd.push(P1_SELECT_BY_NAME);
    cmd.push(0x00);
    cmd.push(AID.len() as u8);
    cmd.extend_from_slice(AID);
    cmd
}

/// Builds a single short EXCHANGE_DATA APDU. Only valid for payloads of
/// at most [`MAX_APDU_DATA`] bytes; [`frame_exchange`] chains longer ones.
pub fn build_exchange_data(payload: &[u8]) -> Vec<u8> {
    build_short(INS_EXCHANGE_DATA, payload)
}

/// Builds a CARD_EXCHANGE APDU command carrying encrypted card data.
pub fn build_card_exchange(encrypted_card: &[u8]) -> Vec<u8> {
    build_short(INS_CARD_EXCHANGE, encrypted_card)
}

fn build_short(ins: u8, data: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(HEADER_LEN + SHORT_LC_LEN + data.len());
    cmd.push(0x00);
    cmd.push(ins);
    cmd.push(0x00);
    cmd.push(0x00);
    cmd.push(data.len() as u8);
    cmd.extend_from_slice(data);
    cmd
}

fn check_ceiling(len: usize) -> Result<(), ApduError> {
    if len > MAX_EXCHANGE_PAYLOAD {
        return Err(ApduError::Oversized {
            len,
            max: MAX_EXCHANGE_PAYLOAD,
        });
    }
    Ok(())
}

/// Frames `payload` as the EXCHANGE APDU chain a reader transmits after
/// the applet is selected: one short APDU per [`MAX_APDU_DATA`] bytes,
/// the chaining bit set on every chunk but the last.
pub fn frame_exchange(payload: &[u8]) -> Result<Vec<Vec<u8>>, ApduError> {
    check_ceiling(payload.len())?;
    Ok(split_into_chain(INS_EXCHANGE_DATA, payload))
}

/// Frames the reader's opening transmission: SELECT, then the EXCHANGE
/// chain for `payload`.
pub fn frame_command(payload: &[u8]) -> Result<Vec<Vec<u8>>, ApduError> {
    let mut apdus = vec![build_select()];
    apdus.extend(frame_exchange(payload)?);
    Ok(apdus)
}

/// Frames a successful response: `data` followed by [`SW_SUCCESS`].
pub fn frame_response(data: &[u8]) -> Result<Vec<u8>, ApduError> {
    check_ceiling(data.len())?;
    let mut response = Vec::with_capacity(data.len() + 2);
    response.extend_from_slice(data);
    response.extend_from_slice(&SW_SUCCESS);
    Ok(response)
}

/// Frames a data-less response carrying only `sw`.
pub fn status_response(sw: [u8; 2]) -> Vec<u8> {
    sw.to_vec()
}

/// Decodes a raw response APDU into its data, mapping every status word
/// other than [`SW_SUCCESS`] to an error.
pub fn decode_response(bytes: &[u8]) -> Result<Vec<u8>, ApduError> {
    let (data, sw) = parse_response(bytes).ok_or(ApduError::Truncated)?;
    check_ceiling(data.len())?;
    match sw {
        SW_SUCCESS => Ok(data.to_vec()),
        SW_AID_NOT_FOUND => Err(ApduError::AidNotFound),
        SW_CONDITIONS_NOT_SATISFIED => Err(ApduError::ConditionsNotSatisfied),
        SW_WRONG_DATA => Err(ApduError::WrongData),
        [sw1, sw2] => Err(ApduError::UnexpectedStatus(sw1, sw2)),
    }
}

/// Decodes a raw command APDU. Accepts short and extended `Lc`, ignores
/// the CLA apart from its chaining bit, and tolerates a trailing `Le`,
/// so every reader in the field parses identically.
pub fn decode_command(bytes: &[u8]) -> Result<ApduCommand, ApduError> {
    if bytes.len() < HEADER_LEN {
        return Err(ApduError::MalformedCommand);
    }
    let data = command_data(bytes)?;
    check_ceiling(data.len())?;
    match bytes[1] {
        INS_SELECT => {
            if bytes[2] == P1_SELECT_BY_NAME && data == AID {
                Ok(ApduCommand::Select)
            } else {
                Err(ApduError::AidNotFound)
            }
        }
        INS_EXCHANGE_DATA => Ok(ApduCommand::Exchange {
            data: data.to_vec(),
            more: bytes[0] & CLA_CHAINING_BIT != 0,
        }),
        ins => Ok(ApduCommand::Unknown { ins }),
    }
}

fn command_data(bytes: &[u8]) -> Result<&[u8], ApduError> {
    let body = &bytes[HEADER_LEN..];
    let Some(&lc) = body.first() else {
        return Ok(&[]);
    };
    let (start, len) = if lc != 0 {
        (SHORT_LC_LEN, lc as usize)
    } else if body.len() >= EXTENDED_LC_LEN {
        (
            EXTENDED_LC_LEN,
            usize::from(u16::from_be_bytes([body[1], body[2]])),
        )
    } else {
        (SHORT_LC_LEN, 0)
    };
    body.get(start..start + len)
        .ok_or(ApduError::MalformedCommand)
}

/// Accumulates EXCHANGE chunks into one payload, bounded by
/// [`MAX_EXCHANGE_PAYLOAD`]. An overflow discards the partial chain so a
/// hostile reader cannot resume it.
#[derive(Debug, Default)]
pub struct ChainReassembler {
    buffer: Vec<u8>,
}

impl ChainReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one chunk; yields the whole payload once the final chunk
    /// (`more == false`) arrives.
    pub fn push(&mut self, data: &[u8], more: bool) -> Result<Option<Vec<u8>>, ApduError> {
        let len = self.buffer.len() + data.len();
        if let Err(e) = check_ceiling(len) {
            self.buffer.clear();
            return Err(e);
        }
        self.buffer.extend_from_slice(data);
        if more {
            return Ok(None);
        }
        Ok(Some(std::mem::take(&mut self.buffer)))
    }
}

/// Reassembles a complete EXCHANGE chain (as produced by
/// [`frame_exchange`]) back into its payload.
pub fn assemble_chain(apdus: &[Vec<u8>]) -> Result<Vec<u8>, ApduError> {
    let mut reassembler = ChainReassembler::new();
    let mut completed = None;
    for apdu in apdus {
        if completed.is_some() {
            return Err(ApduError::ChainBroken);
        }
        match decode_command(apdu)? {
            ApduCommand::Exchange { data, more } => completed = reassembler.push(&data, more)?,
            _ => return Err(ApduError::ChainBroken),
        }
    }
    completed.ok_or(ApduError::ChainBroken)
}

/// Parses a response APDU and returns (data, status_word).
pub fn parse_response(response: &[u8]) -> Option<(&[u8], [u8; 2])> {
    if response.len() < 2 {
        return None;
    }
    let sw_offset = response.len() - 2;
    let sw: [u8; 2] = [response[sw_offset], response[sw_offset + 1]];
    let data = &response[..sw_offset];
    Some((data, sw))
}

/// Parses a command APDU and returns (INS, P1, P2, data).
pub fn parse_command(cmd: &[u8]) -> Option<(u8, u8, u8, &[u8])> {
    if cmd.len() < HEADER_LEN {
        return None;
    }
    let ins = cmd[1];
    let p1 = cmd[2];
    let p2 = cmd[3];
    let data = if cmd.len() > 5 {
        let lc = cmd[4] as usize;
        if cmd.len() >= 5 + lc {
            &cmd[5..5 + lc]
        } else {
            &[]
        }
    } else {
        &[]
    };
    Some((ins, p1, p2, data))
}

/// Checks if the command is a SELECT for our AID.
pub fn is_select_vauchi(cmd: &[u8]) -> bool {
    matches!(parse_command(cmd), Some((INS_SELECT, P1_SELECT_BY_NAME, _, data)) if data == AID)
}

/// Checks if the command is an EXCHANGE_DATA command.
pub fn is_exchange_data(cmd: &[u8]) -> bool {
    matches!(parse_command(cmd), Some((INS_EXCHANGE_DATA, ..)))
}

/// Checks if the command is a CARD_EXCHANGE command.
pub fn is_card_exchange(cmd: &[u8]) -> bool {
    matches!(parse_command(cmd), Some((INS_CARD_EXCHANGE, ..)))
}

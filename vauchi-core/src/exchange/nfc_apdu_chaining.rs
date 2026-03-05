// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! APDU Command Chaining (ISO 7816)
//!
//! Splits large payloads across multiple APDU commands using the chaining bit
//! in the CLA byte. Non-final chunks have CLA bit 4 set (0x10), final chunk
//! uses CLA 0x00.

use super::ExchangeError;

/// Maximum data bytes per APDU command (single-byte Lc field).
pub const MAX_APDU_DATA: usize = 255;

/// CLA byte with chaining bit set (non-final chunk).
const CLA_CHAINING: u8 = 0x10;

/// CLA byte for final chunk.
const CLA_FINAL: u8 = 0x00;

/// Splits data into APDU command chain.
///
/// Each command has format: CLA(1) | INS(1) | P1(1) | P2(1) | Lc(1) | Data(Lc)
/// Non-final commands use CLA=0x10, final uses CLA=0x00.
pub fn split_into_chain(ins: u8, data: &[u8]) -> Vec<Vec<u8>> {
    if data.is_empty() {
        let cmd = vec![CLA_FINAL, ins, 0x00, 0x00, 0x00];
        return vec![cmd];
    }

    let chunks: Vec<&[u8]> = data.chunks(MAX_APDU_DATA).collect();
    let last_idx = chunks.len() - 1;

    chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let cla = if i == last_idx {
                CLA_FINAL
            } else {
                CLA_CHAINING
            };
            let mut cmd = Vec::with_capacity(5 + chunk.len());
            cmd.push(cla);
            cmd.push(ins);
            cmd.push(0x00); // P1
            cmd.push(0x00); // P2
            cmd.push(chunk.len() as u8);
            cmd.extend_from_slice(chunk);
            cmd
        })
        .collect()
}

/// Checks if an APDU command has the chaining bit set (non-final).
pub fn is_chained(cmd: &[u8]) -> bool {
    !cmd.is_empty() && (cmd[0] & 0x10) != 0
}

/// Extracts data from an APDU command (skips 5-byte header).
pub fn extract_data(cmd: &[u8]) -> Result<&[u8], ExchangeError> {
    if cmd.len() < 5 {
        return Err(ExchangeError::NfcChainReassemblyFailed);
    }
    let lc = cmd[4] as usize;
    if cmd.len() < 5 + lc {
        return Err(ExchangeError::NfcChainReassemblyFailed);
    }
    Ok(&cmd[5..5 + lc])
}

/// Reassembles data from a sequence of chained APDU commands.
///
/// Validates that all non-final commands have the chaining bit set and
/// the final command does not.
pub fn reassemble_chain(commands: &[Vec<u8>]) -> Result<Vec<u8>, ExchangeError> {
    if commands.is_empty() {
        return Err(ExchangeError::NfcChainReassemblyFailed);
    }

    let last_idx = commands.len() - 1;
    let mut result = Vec::new();

    for (i, cmd) in commands.iter().enumerate() {
        if i < last_idx && !is_chained(cmd) {
            return Err(ExchangeError::NfcChainReassemblyFailed);
        }
        if i == last_idx && is_chained(cmd) {
            return Err(ExchangeError::NfcChainReassemblyFailed);
        }
        let data = extract_data(cmd)?;
        result.extend_from_slice(data);
    }

    Ok(result)
}

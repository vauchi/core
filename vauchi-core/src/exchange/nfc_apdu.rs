// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ISO 7816-4 APDU framing for the Vauchi NFC exchange.

/// Vauchi NFC Application Identifier (AID).
/// F0 56 41 55 43 48 49 01 — "VAUCHI" with prefix F0 and version 01.
pub const AID: &[u8] = &[0xF0, 0x56, 0x41, 0x55, 0x43, 0x48, 0x49, 0x01];

/// ISO 7816 status word indicating successful command execution (SW1=90, SW2=00).
pub const SW_SUCCESS: [u8; 2] = [0x90, 0x00];
/// ISO 7816 status word indicating the requested AID was not found on the card (SW1=6A, SW2=82).
pub const SW_AID_NOT_FOUND: [u8; 2] = [0x6A, 0x82];
/// ISO 7816 status word indicating conditions of use not satisfied (SW1=69, SW2=85).
pub const SW_CONDITIONS_NOT_SATISFIED: [u8; 2] = [0x69, 0x85];

const INS_SELECT: u8 = 0xA4;
const INS_EXCHANGE_DATA: u8 = 0xE0;
const INS_CARD_EXCHANGE: u8 = 0xE2;

/// Builds a SELECT APDU command for the Vauchi AID.
pub fn build_select() -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5 + AID.len());
    cmd.push(0x00); // CLA
    cmd.push(INS_SELECT); // INS
    cmd.push(0x04); // P1: select by name
    cmd.push(0x00); // P2
    cmd.push(AID.len() as u8); // Lc
    cmd.extend_from_slice(AID);
    cmd
}

/// Builds an EXCHANGE_DATA APDU command carrying our NFC payload.
pub fn build_exchange_data(payload: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5 + payload.len());
    cmd.push(0x00); // CLA
    cmd.push(INS_EXCHANGE_DATA); // INS
    cmd.push(0x00); // P1
    cmd.push(0x00); // P2
    cmd.push(payload.len() as u8); // Lc
    cmd.extend_from_slice(payload);
    cmd
}

/// Builds a CARD_EXCHANGE APDU command carrying encrypted card data.
pub fn build_card_exchange(encrypted_card: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5 + encrypted_card.len());
    cmd.push(0x00); // CLA
    cmd.push(INS_CARD_EXCHANGE); // INS
    cmd.push(0x00); // P1
    cmd.push(0x00); // P2
    cmd.push(encrypted_card.len() as u8); // Lc
    cmd.extend_from_slice(encrypted_card);
    cmd
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
    if cmd.len() < 4 {
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
    if let Some((ins, p1, _p2, data)) = parse_command(cmd) {
        ins == INS_SELECT && p1 == 0x04 && data == AID
    } else {
        false
    }
}

/// Checks if the command is an EXCHANGE_DATA command.
pub fn is_exchange_data(cmd: &[u8]) -> bool {
    if let Some((ins, _, _, _)) = parse_command(cmd) {
        ins == INS_EXCHANGE_DATA
    } else {
        false
    }
}

/// Checks if the command is a CARD_EXCHANGE command.
pub fn is_card_exchange(cmd: &[u8]) -> bool {
    if let Some((ins, _, _, _)) = parse_command(cmd) {
        ins == INS_CARD_EXCHANGE
    } else {
        false
    }
}

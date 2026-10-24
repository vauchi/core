// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::ble::VAUCHI_BLE_SERVICE_UUID;
use uuid::Uuid;

/// Decode the two-byte carrier used by current peers and the legacy
/// four-byte carrier. Ambiguous advertisements must not select a peer by
/// the host API's UUID iteration order (#770).
pub fn advertised_token(service_uuids: &[[u8; 16]]) -> Option<Vec<u8>> {
    if service_uuids.len() > 32 {
        return None;
    }
    let service = Uuid::parse_str(VAUCHI_BLE_SERVICE_UUID).ok()?;
    if !service_uuids.contains(service.as_bytes()) {
        return None;
    }
    let mut result = None;
    for bytes in service_uuids {
        let uuid = Uuid::from_bytes(*bytes);
        let (value, middle, version, tail) = uuid.as_fields();
        if middle != 0 || version != 0x1000 || tail != &[0x80, 0, 0, 0x80, 0x5f, 0x9b, 0x34, 0xfb] {
            continue;
        }
        let token: Vec<u8> = value
            .to_be_bytes()
            .into_iter()
            .skip(if value <= u16::MAX.into() { 2 } else { 0 })
            .collect();
        if result.as_ref().is_some_and(|previous| previous != &token) {
            return None;
        }
        result = Some(token);
    }
    result
}

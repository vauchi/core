// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::ble::VAUCHI_BLE_SERVICE_UUID;
use crate::platform::BleServiceUuid;
use uuid::Uuid;

/// HKDF info for the attempt token a Glance displayer advertises: derived
/// from its QR's random token so the scanner can expect the same prefix.
const ATTEMPT_TOKEN_INFO: &[u8] = b"vauchi-ble-attempt-token-v1";

/// A fresh role-tiebreak token for one BLE exchange screen. The phones put
/// its first bytes on the air as a service UUID; a value derived from a
/// long-lived key would let a bystander re-recognise the device across
/// sightings (vauchi/private#726), so it is random per attempt and never
/// carries role or trust (2026-07-22 tiebreak design, rule 1).
pub fn fresh_attempt_token() -> [u8; 32] {
    crate::crypto::random_bytes()
}

/// The attempt token a Glance displayer advertises, derived from the random
/// token in its QR: the scanner computes the same value from the QR it
/// scanned and connects only to the advertiser whose prefix matches.
pub fn attempt_token_from_oob(qr_token: &[u8; 32]) -> [u8; 32] {
    *crate::crypto::kdf::HKDF::derive_key(None, qr_token, ATTEMPT_TOKEN_INFO)
}

/// Decode the two-byte carrier used by current peers and the legacy
/// four-byte carrier. Ambiguous advertisements must not select a peer by
/// the host API's UUID iteration order (#770).
/// Normalized UUIDs retain Android's current two-byte convention for small
/// values: a leading-zero legacy four-byte carrier is indistinguishable
/// once the OS discards width. Hosts exposing width must preserve it.
pub fn advertised_token(service_uuids: &[BleServiceUuid]) -> Option<Vec<u8>> {
    if service_uuids.len() > 32 {
        return None;
    }
    let service = Uuid::parse_str(VAUCHI_BLE_SERVICE_UUID).ok()?;
    if !service_uuids.iter().any(|uuid| matches!(uuid,
        BleServiceUuid::Uuid128(bytes) | BleServiceUuid::Normalized(bytes) if bytes == service.as_bytes())) {
        return None;
    }
    let mut result = None;
    for uuid in service_uuids {
        let token = match uuid {
            BleServiceUuid::Uuid16(bytes) => bytes.to_vec(),
            BleServiceUuid::Uuid32(bytes) => bytes.to_vec(),
            BleServiceUuid::Uuid128(_) => continue,
            BleServiceUuid::Normalized(bytes) => {
                let uuid = Uuid::from_bytes(*bytes);
                let (value, middle, version, tail) = uuid.as_fields();
                if middle != 0
                    || version != 0x1000
                    || tail != &[0x80, 0, 0, 0x80, 0x5f, 0x9b, 0x34, 0xfb]
                {
                    continue;
                }
                value
                    .to_be_bytes()
                    .into_iter()
                    .skip(if value <= u16::MAX.into() { 2 } else { 0 })
                    .collect()
            }
        };
        if result.as_ref().is_some_and(|previous| previous != &token) {
            return None;
        }
        result = Some(token);
    }
    result
}

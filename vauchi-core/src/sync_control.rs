// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage-independent admission for registry-control wire frames.

use crate::identity::{RegistryBroadcast, device::MAX_REGISTRY_BROADCAST_JSON_BYTES};

pub(crate) const PUSH_VERSION: u8 = 0x05;
pub(crate) const ACK_VERSION: u8 = 0x06;
pub(crate) const NONCE_LEN: usize = 32;
pub(crate) const ACK_PREFIX_LEN: usize = NONCE_LEN + 8 + 1;

pub(crate) fn is_registry_control(payload: &[u8]) -> bool {
    let Some((&version, body)) = payload.split_first() else {
        return false;
    };
    match version {
        PUSH_VERSION => body.get(NONCE_LEN..).is_some_and(valid_broadcast),
        ACK_VERSION => {
            let Some((prefix, broadcast)) = body.split_first_chunk::<ACK_PREFIX_LEN>() else {
                return false;
            };
            match (prefix.last().copied(), broadcast.is_empty()) {
                (Some(0), true) => true,
                (Some(1), false) => valid_broadcast(broadcast),
                _ => false,
            }
        }
        _ => false,
    }
}

fn valid_broadcast(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_REGISTRY_BROADCAST_JSON_BYTES {
        return false;
    }
    std::str::from_utf8(bytes)
        .ok()
        .is_some_and(|json| RegistryBroadcast::from_json(json).is_ok())
}

// INLINE_TEST_REQUIRED: the private storage-free admission boundary is also
// checked against the storage-backed wire decoder; test bodies stay in tests/.
#[cfg(test)]
#[path = "../tests/unit/sync_control_tests.rs"]
mod tests;

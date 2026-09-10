// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Peer display-name decode for the multi-stage machine's `Finalized`
//! transition. Split out of `multi_stage_machine.rs` when that file
//! crossed the file-size threshold (`_private/docs/backlog/
//! 2026-09-10-exchange-stall-and-ble-fallback-states/README.md`) — pure
//! byte-decode logic with no state, so it splits cleanly. Loaded via
//! `#[path]` from the parent; stays a private child module (`super::`
//! item access preserved).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::MultiStageSession;

/// Decode the peer's display name from the just-received
/// exchange payload. Called only on the `Finalized` transition;
/// returns an empty string when the payload is absent (race —
/// Finalized observed before reassembly completes) or malformed
/// (deserialize failure — surfaces as the empty success-chrome
/// name, never panics).
///
/// Wire format mirrors `serialize_exchange_payload`
/// (`vauchi-app/src/ui/app_engine/multi_stage_exchange.rs`):
/// `[version: 1][public_key: 32][card_json: rest]`. Drops the
/// public key after the version check — the contact's signing
/// key lives in storage via the persistence path, not on the
/// success screen.
pub(super) fn extract_peer_name(session: &MultiStageSession) -> String {
    let Some(data) = session.get_received_data() else {
        return String::new();
    };
    decode_peer_name_from_payload(&data)
}

/// Pure-byte counterpart of [`extract_peer_name`] split out so the
/// payload-shape edges (short input, wrong version, malformed
/// `card_json`) can be unit-tested without spinning up a full
/// `MultiStageSession` peer exchange.
fn decode_peer_name_from_payload(data: &[u8]) -> String {
    if data.len() < 34 || data[0] != super::EXCHANGE_PAYLOAD_VERSION {
        return String::new();
    }
    match serde_json::from_slice::<ContactCard>(&data[33..]) {
        Ok(card) => card.display_name().to_string(),
        Err(_) => String::new(),
    }
}

// INLINE_TEST_REQUIRED: decode_peer_name_from_payload is a private
// helper; its edges (short input, wrong version, malformed card_json)
// only exercise here.
#[cfg(test)]
mod peer_name_tests {
    use super::decode_peer_name_from_payload;
    use crate::orchestrator::multi_stage_machine::EXCHANGE_PAYLOAD_VERSION;
    use vauchi_core::contact_card::ContactCard;

    fn build_payload(card: &ContactCard) -> Vec<u8> {
        let json = serde_json::to_vec(card).expect("serialize card");
        let mut out = Vec::with_capacity(1 + 32 + json.len());
        out.push(EXCHANGE_PAYLOAD_VERSION);
        out.extend_from_slice(&[0xAB; 32]);
        out.extend_from_slice(&json);
        out
    }

    // @internal
    #[test]
    fn well_formed_payload_returns_card_display_name() {
        let card = ContactCard::new("Alice");
        let payload = build_payload(&card);
        assert_eq!(decode_peer_name_from_payload(&payload), "Alice");
    }

    // @internal
    #[test]
    fn empty_payload_returns_empty_string() {
        assert_eq!(decode_peer_name_from_payload(&[]), "");
    }

    // @internal
    #[test]
    fn payload_shorter_than_header_returns_empty_string() {
        let short = vec![EXCHANGE_PAYLOAD_VERSION; 20];
        assert_eq!(decode_peer_name_from_payload(&short), "");
    }

    // @internal
    #[test]
    fn unknown_version_byte_returns_empty_string() {
        let card = ContactCard::new("Bob");
        let mut payload = build_payload(&card);
        payload[0] = 0xFF;
        assert_eq!(decode_peer_name_from_payload(&payload), "");
    }

    // @internal
    #[test]
    fn malformed_card_json_returns_empty_string() {
        let mut payload = vec![EXCHANGE_PAYLOAD_VERSION];
        payload.extend_from_slice(&[0xAB; 32]);
        payload.extend_from_slice(b"{not-valid-json");
        assert_eq!(decode_peer_name_from_payload(&payload), "");
    }
}

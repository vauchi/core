// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Forwarding Hint Following
//!
//! When a relay offloads blobs to peer relays (federation), it sends
//! forwarding hints to the recipient client. This module handles parsing
//! those hints and fetching the offloaded blobs from the hinted relays.

use std::collections::HashSet;

use super::message::{ForwardingHint, ForwardingHints, MessageEnvelope};

/// Filters out expired hints based on the current time.
pub fn filter_expired_hints(hints: &ForwardingHints, now_secs: u64) -> Vec<&ForwardingHint> {
    hints
        .hints
        .iter()
        .filter(|h| h.expires_at_secs > now_secs)
        .collect()
}

/// Deduplicates forwarding hints by blob_id, keeping the first occurrence.
pub fn deduplicate_hints(hints: &[ForwardingHint]) -> Vec<&ForwardingHint> {
    let mut seen = HashSet::new();
    hints
        .iter()
        .filter(|h| seen.insert(h.blob_id.as_str()))
        .collect()
}

/// Deduplicates received message envelopes by message_id.
pub fn deduplicate_envelopes(envelopes: Vec<MessageEnvelope>) -> Vec<MessageEnvelope> {
    let mut seen = HashSet::new();
    envelopes
        .into_iter()
        .filter(|e| seen.insert(e.message_id.clone()))
        .collect()
}

/// Groups forwarding hints by relay URL for batch fetching.
pub fn group_hints_by_relay<'a>(
    hints: &[&'a ForwardingHint],
) -> Vec<(&'a str, Vec<&'a ForwardingHint>)> {
    let mut groups: Vec<(&'a str, Vec<&'a ForwardingHint>)> = Vec::new();

    for hint in hints {
        if let Some(group) = groups.iter_mut().find(|(url, _)| *url == hint.relay_url) {
            group.1.push(hint);
        } else {
            groups.push((hint.relay_url.as_str(), vec![hint]));
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::message::{
        ForwardingHint, ForwardingHints, MessageEnvelope, MessagePayload, PROTOCOL_VERSION,
    };

    fn make_hint(blob_id: &str, relay_url: &str, expires_at: u64) -> ForwardingHint {
        ForwardingHint {
            blob_id: blob_id.to_string(),
            relay_url: relay_url.to_string(),
            expires_at_secs: expires_at,
        }
    }

    #[test]
    fn test_filter_expired_hints() {
        let hints = ForwardingHints {
            hints: vec![
                make_hint("blob-1", "wss://relay-a.test", 1000),
                make_hint("blob-2", "wss://relay-b.test", 2000),
                make_hint("blob-3", "wss://relay-a.test", 500),
            ],
        };

        let active = filter_expired_hints(&hints, 800);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].blob_id, "blob-1");
        assert_eq!(active[1].blob_id, "blob-2");
    }

    #[test]
    fn test_filter_all_expired() {
        let hints = ForwardingHints {
            hints: vec![
                make_hint("blob-1", "wss://relay-a.test", 100),
                make_hint("blob-2", "wss://relay-b.test", 200),
            ],
        };

        let active = filter_expired_hints(&hints, 300);
        assert!(active.is_empty());
    }

    #[test]
    fn test_filter_none_expired() {
        let hints = ForwardingHints {
            hints: vec![
                make_hint("blob-1", "wss://relay-a.test", 1000),
                make_hint("blob-2", "wss://relay-b.test", 2000),
            ],
        };

        let active = filter_expired_hints(&hints, 0);
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_deduplicate_hints() {
        let hints = vec![
            make_hint("blob-1", "wss://relay-a.test", 1000),
            make_hint("blob-1", "wss://relay-b.test", 2000), // duplicate blob_id
            make_hint("blob-2", "wss://relay-a.test", 1500),
        ];

        let deduped = deduplicate_hints(&hints);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].blob_id, "blob-1");
        assert_eq!(deduped[0].relay_url, "wss://relay-a.test"); // first occurrence kept
        assert_eq!(deduped[1].blob_id, "blob-2");
    }

    #[test]
    fn test_deduplicate_envelopes() {
        let envelopes = vec![
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-1".to_string(),
                timestamp: 100,
                payload: MessagePayload::ForwardingHints(ForwardingHints { hints: vec![] }),
            },
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-1".to_string(), // duplicate
                timestamp: 200,
                payload: MessagePayload::ForwardingHints(ForwardingHints { hints: vec![] }),
            },
            MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "msg-2".to_string(),
                timestamp: 300,
                payload: MessagePayload::ForwardingHints(ForwardingHints { hints: vec![] }),
            },
        ];

        let deduped = deduplicate_envelopes(envelopes);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].message_id, "msg-1");
        assert_eq!(deduped[1].message_id, "msg-2");
    }

    #[test]
    fn test_group_hints_by_relay() {
        let hints = vec![
            make_hint("blob-1", "wss://relay-a.test", 1000),
            make_hint("blob-2", "wss://relay-b.test", 1000),
            make_hint("blob-3", "wss://relay-a.test", 1000),
            make_hint("blob-4", "wss://relay-b.test", 1000),
            make_hint("blob-5", "wss://relay-c.test", 1000),
        ];

        let hint_refs: Vec<&ForwardingHint> = hints.iter().collect();
        let groups = group_hints_by_relay(&hint_refs);

        assert_eq!(groups.len(), 3);

        // Find relay-a group
        let relay_a = groups
            .iter()
            .find(|(url, _)| *url == "wss://relay-a.test")
            .unwrap();
        assert_eq!(relay_a.1.len(), 2);

        // Find relay-b group
        let relay_b = groups
            .iter()
            .find(|(url, _)| *url == "wss://relay-b.test")
            .unwrap();
        assert_eq!(relay_b.1.len(), 2);

        // Find relay-c group
        let relay_c = groups
            .iter()
            .find(|(url, _)| *url == "wss://relay-c.test")
            .unwrap();
        assert_eq!(relay_c.1.len(), 1);
    }

    #[test]
    fn test_empty_hints() {
        let hints = ForwardingHints { hints: vec![] };
        let active = filter_expired_hints(&hints, 0);
        assert!(active.is_empty());

        let deduped = deduplicate_hints(&[]);
        assert!(deduped.is_empty());
    }

    #[test]
    fn test_forwarding_hints_serde_roundtrip() {
        let hints = ForwardingHints {
            hints: vec![
                make_hint("blob-1", "wss://relay-a.test", 1000),
                make_hint("blob-2", "wss://relay-b.test", 2000),
            ],
        };

        let json = serde_json::to_string(&hints).unwrap();
        let deserialized: ForwardingHints = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.hints.len(), 2);
        assert_eq!(deserialized.hints[0].blob_id, "blob-1");
        assert_eq!(deserialized.hints[1].relay_url, "wss://relay-b.test");
    }

    #[test]
    fn test_forwarding_hints_in_envelope() {
        let envelope = MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "test-fwd-1".to_string(),
            timestamp: 1700000000,
            payload: MessagePayload::ForwardingHints(ForwardingHints {
                hints: vec![make_hint("blob-1", "wss://relay-a.test", 1000)],
            }),
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let deserialized: MessageEnvelope = serde_json::from_str(&json).unwrap();

        match deserialized.payload {
            MessagePayload::ForwardingHints(fh) => {
                assert_eq!(fh.hints.len(), 1);
                assert_eq!(fh.hints[0].blob_id, "blob-1");
            }
            _ => panic!("Expected ForwardingHints variant"),
        }
    }
}

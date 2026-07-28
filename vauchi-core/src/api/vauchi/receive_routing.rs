// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Receive-phase blob routing for **contact card updates**.
//!
//! Owns the per-blob routing logic that `sync_http::run_receive_phase`
//! delegates to: build a `mailbox_token → contact_id` map from the local
//! contact list (O(N) HKDF), then resolve each fetched blob in O(1) via
//! `FetchedBlob.mailbox_token` (ADR-029 addendum 2026-04-27).
//!
//! Step 2 of the receive-phase-token-attribution plan removed the
//! brute-force fallback. The relay now always populates `mailbox_token`
//! (deployed 2026-04-27). Blobs whose token doesn't appear in the local
//! map cannot be card updates from any contact we know — the recipient
//! registered the token, so a blob arriving for it must come from a
//! contact whose token we just computed. Anything else is malformed or
//! out-of-protocol; ACK as `Stored` and move on.
//!
//! ## Scope: contact updates only
//!
//! Device-sync inbound (blobs sent to `compute_self_token(master_seed)`,
//! see `network/relay_client.rs::send_device_sync_message`) is **not**
//! handled here. The recipient registers self-tokens via
//! `batch_register_tokens`, so the relay may return self-token blobs in
//! the same fetch response — this function returns `TokenUnresolved`
//! for them and the receive loop ACKs them as `Stored`. A separate path
//! (`send_phase::process_device_sync` /
//! `sync::device_orchestrator::DeviceSyncOrchestrator`) is responsible
//! for device-sync; this module's `token_to_contact_map` intentionally
//! omits self-tokens.
//!
//! Pure with respect to network I/O — the caller drives ACKs from the
//! returned outcomes. Storage is only mutated on successful decryption
//! via `process_single_card_update`.

use std::collections::HashMap;

use crate::api::events::VauchiEvent;
use crate::api::sync::{
    CardUpdateError, ReceiveOutcome, ReceivedAlert, process_single_card_update,
    process_single_card_update_for_authenticated_device,
};
use crate::contact::Contact;
use crate::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};

/// Opens an origin-device hint (if present) to the sender device id, using the
/// resolved contact's shared key. Returns `None` when there is no hint, the
/// contact has no shared key, or the hint fails to authenticate — the caller
/// then falls back to the legacy `[0;32]` sender-device path.
fn resolve_origin_device(
    contacts: &[Contact],
    contact_id: &str,
    mailbox_token_hex: &str,
    ciphertext: &[u8],
    origin_hint: Option<&str>,
) -> Option<[u8; 32]> {
    let hint = origin_hint?;
    let shared_key = contacts
        .iter()
        .find(|contact| contact.id() == contact_id)?
        .shared_key()?;
    crate::network::origin_hint::open_origin_hint(
        shared_key.as_bytes(),
        mailbox_token_hex,
        ciphertext,
        hint,
    )
}

/// Outcome of processing a single received blob.
///
/// `token_resolved` reports whether the blob's `mailbox_token` matched a
/// contact in the local routing map. Together with `decrypted` this
/// distinguishes three operationally distinct states:
///
/// - `decrypted = true` (implies `token_resolved = true`): card update
///   applied successfully.
/// - `token_resolved = true`, `decrypted = false`: contact found, but
///   `process_single_card_update` rejected the payload (signature,
///   replay, blocked sender, garbage ratchet message, etc.). Indicates
///   ratchet desync or storage state that warrants investigation.
/// - `token_resolved = false`, `decrypted = false`: the blob's token
///   matched no known contact (relay regression, attacker probe, drift
///   beyond ±1-day window, or a self-token forwarded to the wrong path).
///   Indicates a protocol or deployment issue.
#[derive(Debug, Clone)]
pub struct BlobOutcome {
    pub message_id: String,
    pub token_resolved: bool,
    pub decrypted: bool,
    /// When `token_resolved && !decrypted`, the short category of WHY
    /// `process_single_card_update` rejected the blob (e.g. `bad_wire`,
    /// `decrypt`, `signature`, `replay`). Diagnostics for
    /// 2026-06-28-sync-delivery-sent-not-received — the device couldn't be
    /// reproduced in-core, so the failing step must be named on-device.
    pub reject_reason: Option<&'static str>,
    /// The resolved contact id when the blob applied (`decrypted == true`);
    /// `None` for rejected or unresolved blobs. Carries the id out of the
    /// storage-pure routing so `run_receive_phase` can dispatch an
    /// `IncomingUpdate` per applied update — without it the contacts list
    /// never re-renders the synced tile (2026-06-30, ADR-021/043).
    pub contact_id: Option<String>,
    /// Existing contact whose verified card must be queued for linked devices.
    /// Set for an accepted card delta and for its replay retry; alerts and
    /// reciprocity confirmations never populate it.
    pub device_fanout_contact_id: Option<String>,
    /// `Some` when the applied blob was a verified safety alert (not a card
    /// delta). Drives an `Emergency`/`DuressAlertReceived` event instead of an
    /// `IncomingUpdate` (2026-07-04-coercion-safety-alerts-never-received).
    pub alert: Option<ReceivedAlert>,
    /// `Some` when the applied blob was an F4 registry push (or an ack
    /// carrying an echo) that requires a reply ack; the receive loop queues
    /// it after processing (ADR-064 Amendment 2026-07-25). Control-plane
    /// only — never drives a user-visible event.
    pub registry_reply: Option<crate::api::sync::RegistryReplyNeeded>,
}

impl BlobOutcome {
    /// Whether the receive loop may ACK this blob to the relay. An ACKed
    /// blob is deleted server-side, so a locally rolled-back receive
    /// (storage failure) must NOT ACK — the blob stays on the relay and the
    /// next fetch retries it. ACKing it would turn a transient local failure
    /// into permanent silent loss (the alert/nonce transaction was rolled
    /// back, so nothing local remembers the blob existed). A rate-limited
    /// genesis attempt is likewise retriable — the blob is retained until the
    /// window resets (plan §REVISION F6).
    pub(crate) fn should_ack(&self) -> bool {
        !matches!(self.reject_reason, Some("storage") | Some("genesis_rate"))
    }
}

/// Route and apply a batch of received blobs.
///
/// Each blob is resolved via its mailbox token in a local
/// `token → contact_id` map (O(1)). Blobs whose token isn't in the map
/// are dropped — the recipient registered every legitimate token, so a
/// non-resolvable token indicates a malformed or out-of-protocol blob.
/// Returns one outcome per input blob in the same order.
///
/// Pure with respect to network I/O — caller is responsible for sending
/// any ACKs derived from the returned outcomes. Mutates `storage` only
/// on successful decryption (see `process_single_card_update`).
///
/// Exposed for integration tests so they can exercise the receive-phase
/// routing logic without spinning up a transport.
pub fn process_received_blobs(
    identity: &crate::identity::Identity,
    storage: &crate::storage::Storage,
    contacts: &[Contact],
    blobs: Vec<(String, String, Vec<u8>, Option<String>)>,
) -> Vec<BlobOutcome> {
    let day = current_day_epoch(storage.clock().unix_seconds());
    let token_to_contact = build_token_to_contact_map(
        contacts,
        identity.signing_public_key(),
        identity.device_id(),
        day,
    );

    let mut outcomes = Vec::with_capacity(blobs.len());
    for (message_id, mailbox_token_hex, ciphertext, origin_hint) in blobs {
        let (
            token_resolved,
            decrypted,
            reject_reason,
            applied_contact_id,
            device_fanout_contact_id,
            alert,
            registry_reply,
        ) = match token_to_contact.get(&mailbox_token_hex) {
            Some(contact_id) => {
                // Open the origin-device hint (if any) to select the sender's
                // device-pair ratchet before decrypting. A hint that fails to
                // open (legacy, wrong contact, swapped, tampered) falls back to
                // the [0;32] path (F4 origin-device hint design).
                let outcome = match resolve_origin_device(
                    contacts,
                    contact_id,
                    &mailbox_token_hex,
                    &ciphertext,
                    origin_hint.as_deref(),
                ) {
                    Some(peer_device_id) => process_single_card_update_for_authenticated_device(
                        identity,
                        storage,
                        contact_id,
                        &peer_device_id,
                        &ciphertext,
                    ),
                    None => process_single_card_update(identity, storage, contact_id, &ciphertext),
                };
                match outcome {
                    Ok(ReceiveOutcome::CardDelta) => (
                        true,
                        true,
                        None,
                        Some(contact_id.clone()),
                        Some(contact_id.clone()),
                        None,
                        None,
                    ),
                    Ok(ReceiveOutcome::Alert(a)) => (
                        true,
                        true,
                        None,
                        Some(contact_id.clone()),
                        None,
                        Some(a),
                        None,
                    ),
                    // F4 handshake state is persisted inside the receive
                    // transaction; the reply ack rides the outcome so the
                    // receive loop can queue it. Control-plane only — no
                    // IncomingUpdate, no fan-out, no user-visible event.
                    Ok(ReceiveOutcome::RegistryPushReceived(reply)) => {
                        (true, true, None, None, None, None, Some(reply))
                    }
                    Ok(ReceiveOutcome::RegistryAckReceived { reply, .. }) => {
                        (true, true, None, None, None, None, reply)
                    }
                    // P3: the signature is verified in
                    // `process_single_card_update`; match the carried token
                    // against the contact's derived `expected_their_token`
                    // and, on a match, resolve to Confirmed (RelaySync). A
                    // mismatch leaves Pending (fail-safe) — a valid signature
                    // over a wrong token never confirms. On confirm, surface
                    // it like a card update so the frontend reloads and the
                    // reciprocity banner clears.
                    Ok(ReceiveOutcome::ReciprocityConfirm { sender_id, token }) => {
                        if resolve_reciprocity_confirm(identity, storage, &sender_id, &token) {
                            (true, true, None, Some(sender_id), None, None, None)
                        } else {
                            (true, true, None, None, None, None, None)
                        }
                    }
                    Err(e) => {
                        let reason = reject_category(&e);
                        let retry_fanout = (reason == "replay").then(|| contact_id.clone());
                        (true, false, Some(reason), None, retry_fanout, None, None)
                    }
                }
            }
            None => (false, false, None, None, None, None, None),
        };
        outcomes.push(BlobOutcome {
            message_id,
            token_resolved,
            decrypted,
            reject_reason,
            contact_id: applied_contact_id,
            device_fanout_contact_id,
            alert,
            registry_reply,
        });
    }
    outcomes
}

/// P3: resolve a signature-verified reciprocity confirmation. Derives the
/// contact's `expected_their_token` from its shared key (mode-agnostic — no
/// stored token) and constant-time-compares it to the peer's carried token. On
/// a match, records `Confirmed` via `RelaySync` and returns `true`; a mismatch
/// (or an imported/absent contact) leaves state unchanged — fail-safe Pending —
/// and returns `false`. A valid signature over a non-matching token never
/// confirms, so the relay cannot manufacture a false Confirmed (design G1).
fn resolve_reciprocity_confirm(
    identity: &crate::identity::Identity,
    storage: &crate::storage::Storage,
    sender_id: &str,
    token: &[u8; 32],
) -> bool {
    use subtle::ConstantTimeEq;

    let Ok(Some(mut contact)) = storage.contacts().load_contact(sender_id) else {
        return false;
    };
    let Some((_our, expected)) = contact.derive_reciprocity_tokens(identity.signing_public_key())
    else {
        return false;
    };
    if !bool::from(token.as_slice().ct_eq(expected.as_slice())) {
        return false;
    }
    contact.set_reciprocity(crate::exchange::reciprocity::Reciprocity::Confirmed);
    contact.set_confirmation_channel(crate::exchange::reciprocity::ConfirmationChannel::RelaySync);
    storage.contacts().save_contact(&contact).is_ok()
}

/// Map applied receive outcomes to `IncomingUpdate` events.
///
/// One event per applied blob (`contact_id == Some`); rejected and
/// unresolved blobs yield nothing. `affected_screens` maps `IncomingUpdate`
/// to `["contacts", "contact_detail"]`, so dispatching these is what makes
/// the frontend reload the contacts list after a relay-synced peer card
/// update (ADR-021/043). Pure: storage stays in [`process_received_blobs`],
/// dispatch stays in `run_receive_phase`. Duplicate per-contact
/// invalidations are idempotent, so no de-duplication is needed.
///
/// Alert outcomes yield nothing here: alert events are dispatched exclusively
/// from durable facts (`surface_pending_safety_alerts`), so a crash between
/// receive-commit and dispatch can never lose one.
pub fn incoming_update_events(outcomes: &[BlobOutcome]) -> Vec<VauchiEvent> {
    outcomes
        .iter()
        .filter_map(|outcome| {
            let contact_id = outcome.contact_id.clone()?;
            match &outcome.alert {
                Some(_) => None,
                None => Some(VauchiEvent::IncomingUpdate { contact_id }),
            }
        })
        .collect()
}

/// Short, PII-free category for a rejected blob — names WHICH receive step
/// failed so the device can report it (logging-rules: no payload contents).
/// `bad_wire` = the serialized RatchetMessage didn't parse (the wire-header
/// class, core!1201); `decrypt` = ratchet decrypt failed (key/state);
/// `signature` = delta signature mismatch; `replay`/`stale` = already applied.
fn reject_category(e: &CardUpdateError) -> &'static str {
    match e {
        CardUpdateError::SenderRevoked => "revoked",
        CardUpdateError::ContactNotFound => "no_contact",
        CardUpdateError::ContactBlocked => "blocked",
        CardUpdateError::NoRatchetState => "no_ratchet",
        CardUpdateError::InvalidRatchetMessage => "bad_wire",
        CardUpdateError::DecryptionFailed => "decrypt",
        CardUpdateError::InvalidPayload(_) => "bad_payload",
        CardUpdateError::CekDecryptionFailed => "cek",
        CardUpdateError::InvalidDelta => "bad_delta",
        CardUpdateError::SignatureInvalid => "signature",
        CardUpdateError::ReplayDetected => "replay",
        CardUpdateError::GenesisRateLimited => "genesis_rate",
        // Deterministic integrity conflict — ACKable (not "storage"/"genesis_rate"),
        // since retrying the same blob can never resolve it (F9).
        CardUpdateError::FactConflict => "fact_conflict",
        CardUpdateError::StaleVersion { .. } => "stale",
        CardUpdateError::DeltaApplicationFailed => "delta_apply",
        CardUpdateError::Storage(_) => "storage",
    }
}

/// Build a `mailbox_token → contact_id` lookup map for the receive loop.
///
/// Includes the current day's token plus the previous day's token to absorb
/// clock-skew at the daily rotation boundary, matching the registration
/// window in `batch_register_tokens`. Skips contacts that are not exchanged
/// or are blocked — those cannot legitimately decrypt incoming updates.
///
/// O(N) HKDF derivations per call, where N = number of exchanged
/// non-blocked contacts. Tokens are hex-encoded so they compare directly
/// against `FetchedBlob.mailbox_token`.
pub(crate) fn build_token_to_contact_map(
    contacts: &[Contact],
    own_pubkey: &[u8; 32],
    own_device_id: &[u8; 32],
    day: u64,
) -> HashMap<String, String> {
    use crate::network::mailbox_token::compute_device_mailbox_token;
    let mut map = HashMap::new();
    for contact in contacts {
        if !contact.is_exchanged() || contact.is_blocked() {
            continue;
        }
        let Some(shared_key) = contact.shared_key() else {
            continue;
        };
        let bytes = shared_key.as_bytes();
        let id = contact.id().to_string();
        // Our directional RECEIVE token from this contact, keyed to our own
        // identity — so we never resolve (and decrypt-fail) our own sends.
        map.insert(
            token_hex(&compute_mailbox_token(bytes, own_pubkey, day)),
            id.clone(),
        );
        // F4: also resolve our DEVICE-scoped receive token so per-device
        // fan-out copies addressed to THIS device route to the contact
        // (ADR-064 Amendment 2026-07-25).
        map.insert(
            token_hex(&compute_device_mailbox_token(
                bytes,
                own_pubkey,
                own_device_id,
                day,
            )),
            id.clone(),
        );
        if day > 0 {
            map.insert(
                token_hex(&compute_mailbox_token(bytes, own_pubkey, day - 1)),
                id.clone(),
            );
            map.insert(
                token_hex(&compute_device_mailbox_token(
                    bytes,
                    own_pubkey,
                    own_device_id,
                    day - 1,
                )),
                id,
            );
        }
    }
    map
}

// INLINE_TEST_REQUIRED: build_token_to_contact_map is pub(crate); tests/it/
// would need an additional crate-level re-export. Inline keeps the helper
// and its unit tests together — the integration coverage in
// tests/it/sync_receive_routing_tests.rs exercises the public surface.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymmetricKey;
    use crate::contact_card::ContactCard;
    use crate::sync::safety_alert::AlertKind;

    fn exchanged_contact(label: &str) -> Contact {
        let pk = [label.as_bytes()[0]; 32];
        let shared = SymmetricKey::generate();
        Contact::from_exchange(pk, ContactCard::new(label), shared, 0)
    }

    // Our (the receiver's) identity key — directional tokens are keyed to it,
    // so the injected token must use the SAME key the map derives with.
    const OWN_PK: [u8; 32] = [0xAA; 32];
    const OWN_DEVICE: [u8; 32] = [0xBB; 32];

    // @scenario: receive_phase :: token map resolves contact mailbox tokens
    #[test]
    fn test_build_token_to_contact_map_resolves_today_and_yesterday() {
        let alice = exchanged_contact("A");
        let bob = exchanged_contact("B");
        let day = 12345;
        let alice_today = token_hex(&compute_mailbox_token(
            alice.shared_key().unwrap().as_bytes(),
            &OWN_PK,
            day,
        ));
        let alice_yesterday = token_hex(&compute_mailbox_token(
            alice.shared_key().unwrap().as_bytes(),
            &OWN_PK,
            day - 1,
        ));
        let bob_today = token_hex(&compute_mailbox_token(
            bob.shared_key().unwrap().as_bytes(),
            &OWN_PK,
            day,
        ));

        let contacts = vec![alice.clone(), bob.clone()];
        let map = build_token_to_contact_map(&contacts, &OWN_PK, &OWN_DEVICE, day);

        assert_eq!(map.get(&alice_today).map(String::as_str), Some(alice.id()));
        assert_eq!(
            map.get(&alice_yesterday).map(String::as_str),
            Some(alice.id()),
            "previous-day token must resolve for clock-skew tolerance"
        );
        assert_eq!(map.get(&bob_today).map(String::as_str), Some(bob.id()));
    }

    // @scenario: receive_phase :: token map omits blocked contacts
    #[test]
    fn test_build_token_to_contact_map_skips_blocked_contacts() {
        let mut alice = exchanged_contact("A");
        alice.set_blocked(true);
        let day = 9999;
        let alice_today = token_hex(&compute_mailbox_token(
            alice.shared_key().unwrap().as_bytes(),
            &OWN_PK,
            day,
        ));

        let map = build_token_to_contact_map(&[alice], &OWN_PK, &OWN_DEVICE, day);

        assert!(
            !map.contains_key(&alice_today),
            "blocked contacts must not appear in the token-routing map"
        );
    }

    // @scenario: receive_phase :: token map yields O(1) lookup for unknown tokens
    #[test]
    fn test_build_token_to_contact_map_returns_none_for_unknown_token() {
        let alice = exchanged_contact("A");
        let map = build_token_to_contact_map(&[alice], &OWN_PK, &OWN_DEVICE, 100);
        assert!(!map.contains_key("00".repeat(32).as_str()));
    }

    fn outcome(decrypted: bool, contact_id: Option<&str>) -> BlobOutcome {
        BlobOutcome {
            message_id: "m".into(),
            token_resolved: contact_id.is_some() || !decrypted,
            decrypted,
            reject_reason: if decrypted { None } else { Some("decrypt") },
            contact_id: contact_id.map(str::to_string),
            device_fanout_contact_id: contact_id.map(str::to_string),
            alert: None,
            registry_reply: None,
        }
    }

    // @scenario: receive_phase :: a rolled-back receive must not ACK the blob
    #[test]
    fn test_storage_failure_outcome_is_not_ackable() {
        let mut storage_failed = outcome(false, None);
        storage_failed.reject_reason = Some("storage");
        assert!(
            !storage_failed.should_ack(),
            "an ACKed blob is deleted by the relay — a rolled-back receive \
             must leave it for refetch or the alert is silently lost"
        );

        assert!(outcome(true, Some("c")).should_ack());
        let mut rejected = outcome(false, None);
        rejected.reject_reason = Some("decrypt");
        assert!(
            rejected.should_ack(),
            "non-storage rejects are deterministic — refetching cannot fix \
             them, so they still ACK"
        );
    }

    // @scenario: receive_phase :: an applied update yields one IncomingUpdate
    #[test]
    fn test_incoming_update_events_maps_only_applied_blobs() {
        let outcomes = vec![
            outcome(true, Some("contact-A")),
            outcome(false, None), // rejected: decrypt failed
            BlobOutcome {
                message_id: "m".into(),
                token_resolved: false,
                decrypted: false,
                reject_reason: None,
                contact_id: None, // unresolved token
                device_fanout_contact_id: None,
                alert: None,
                registry_reply: None,
            },
        ];

        let events = incoming_update_events(&outcomes);

        assert_eq!(events.len(), 1, "only the applied blob emits an event");
        match &events[0] {
            VauchiEvent::IncomingUpdate { contact_id } => assert_eq!(contact_id, "contact-A"),
            other => panic!("expected IncomingUpdate, got {other:?}"),
        }
    }

    // @scenario: receive_phase :: alert events derive from durable facts only
    #[test]
    fn test_incoming_update_events_skips_alert_outcomes() {
        let mut applied_alert = outcome(true, Some("contact-A"));
        applied_alert.alert = Some(ReceivedAlert {
            kind: AlertKind::Duress,
            message: "m".into(),
            timestamp: 1,
            location: None,
            nonce: [1u8; 32],
        });
        assert!(
            incoming_update_events(&[applied_alert]).is_empty(),
            "alerts dispatch from surface_pending_safety_alerts, never from outcomes"
        );
    }

    // @scenario: receive_phase :: a batch that applies nothing emits no events
    #[test]
    fn test_incoming_update_events_empty_when_nothing_applied() {
        let outcomes = vec![outcome(false, None)];
        assert!(
            incoming_update_events(&outcomes).is_empty(),
            "a rejected blob must not invalidate the contacts list"
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a locked run does with a contact's blob (ADR-033 amendment
//! 2026-11-07 and addendum 2026-11-09, vauchi/private#1093 slice 6). It
//! opens the blob on a session it already holds, then saves the advanced
//! ratchet and seals the opened update into the inbox in one transaction,
//! so the ratchet never moves past an update that was not kept. Signature,
//! replay and version checks need the contact, under the storage key; the
//! merge at the unlock runs them.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::RatchetMessage;
use crate::storage::{LockedReceiveStorage, StorageError};

/// A fetch page is capped under the OHTTP forward limit, so a larger blob
/// is not one the relay delivered.
const MAX_BLOB_LEN: usize = 128 * 1024;

/// What became of one blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedReceipt {
    /// In the inbox, the ratchet saved with it: the blob can be
    /// acknowledged.
    Stored,
    /// Not opened on any session held, or not a ratchet message: it stays
    /// on the relay for the unlock.
    LeftForUnlock,
}

/// An update a locked run opened, as the inbox keeps it.
#[derive(Serialize, Deserialize)]
pub struct LockedInboxItem {
    contact_id: String,
    peer_device_id: [u8; 32],
    plaintext: Vec<u8>,
    received_at: u64,
}

impl Drop for LockedInboxItem {
    fn drop(&mut self) {
        self.plaintext.zeroize();
    }
}

impl LockedInboxItem {
    /// Reads an item from the bytes the inbox opened.
    pub fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        serde_json::from_slice(bytes).map_err(|e| StorageError::Serialization(e.to_string()))
    }

    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|e| StorageError::Serialization(e.to_string()))
    }

    /// The contact whose session opened it.
    pub fn contact_id(&self) -> &str {
        &self.contact_id
    }

    /// The contact's device whose session opened it; all zeros for the
    /// shared session.
    pub fn peer_device_id(&self) -> &[u8; 32] {
        &self.peer_device_id
    }

    /// The opened update, not yet verified.
    pub fn plaintext(&self) -> &[u8] {
        &self.plaintext
    }

    /// Unix seconds when the locked run received it.
    pub fn received_at(&self) -> u64 {
        self.received_at
    }
}

/// Opens `ciphertext`, routed to `contact_id`, on the first session with
/// that contact that opens it, and keeps it for the unlock.
///
/// A failed trial is not saved, so it advances no session. A session still
/// under the storage key is skipped like one that does not open.
pub fn store_locked_update(
    storage: &LockedReceiveStorage,
    contact_id: &str,
    ciphertext: &[u8],
    received_at: u64,
) -> Result<LockedReceipt, StorageError> {
    if ciphertext.len() > MAX_BLOB_LEN {
        return Ok(LockedReceipt::LeftForUnlock);
    }
    let Ok(message) = serde_json::from_slice::<RatchetMessage>(ciphertext) else {
        return Ok(LockedReceipt::LeftForUnlock);
    };
    for peer_device_id in storage.ratchets().peer_devices(contact_id)? {
        let session = match storage
            .ratchets()
            .load_ratchet_state_for_device(contact_id, &peer_device_id)
        {
            Ok(Some(session)) => session,
            Ok(None) | Err(StorageError::StorageKeyUnavailable) => continue,
            Err(e) => return Err(e),
        };
        let (mut ratchet, is_initiator) = session;
        let Ok(plaintext) = ratchet.decrypt(&message) else {
            continue;
        };
        let item = LockedInboxItem {
            contact_id: contact_id.to_string(),
            peer_device_id,
            plaintext,
            received_at,
        }
        .encode()?;
        storage.in_transaction(|storage| {
            storage.ratchets().save_ratchet_state_for_device(
                contact_id,
                &peer_device_id,
                &ratchet,
                is_initiator,
            )?;
            storage.receive_inbox().put(&item)
        })?;
        return Ok(LockedReceipt::Stored);
    }
    Ok(LockedReceipt::LeftForUnlock)
}

/// What a locked run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedRunOutcome {
    /// No unlocked sync stored today's registration.
    NothingStored,
    /// The day after the last unlocked sync has passed; the unlock catches
    /// up (ADR-033 addendum 2026-11-09, point 3).
    WindowEnded,
    /// The outer relay offers delayed fetches, whose answers a short
    /// background run cannot wait for; the unlock fetches (owner
    /// 2026-11-09).
    DelayedFetchesOffered,
    /// No OHTTP key the gateway still accepts is held; a locked run fetches
    /// none.
    NoOhttpKey,
    /// Fetched: `stored` blobs kept and acknowledged, `left` on the relay
    /// for the unlock.
    Received { stored: usize, left: usize },
}

/// Receives on a locked phone: registers the sets the last unlocked sync
/// stored for yesterday and today, fetches over OHTTP with the held signed
/// key, keeps what a held session opens and acknowledges it. It sends
/// nothing else (ADR-033 amendment 2026-11-07, point 6) and moves no
/// receive checkpoint, so the unlock's catch-up still covers the days.
#[cfg(feature = "network-http")]
pub fn receive_while_locked(
    storage: &LockedReceiveStorage,
    relay: &crate::api::RelayConfig,
    clock: std::sync::Arc<dyn crate::clock::Clock>,
    rng: &dyn crate::rng::SecureRng,
) -> Result<LockedRunOutcome, crate::api::VauchiError> {
    network::receive(storage, relay, clock, rng, None)
}

/// [`receive_while_locked`] with an in-memory OHTTP key, for a relay
/// without an anchor.
#[cfg(all(feature = "network-http", feature = "testing"))]
pub fn receive_while_locked_with_ohttp(
    storage: &LockedReceiveStorage,
    relay: &crate::api::RelayConfig,
    clock: std::sync::Arc<dyn crate::clock::Clock>,
    rng: &dyn crate::rng::SecureRng,
    ohttp: crate::network::OhttpClient,
) -> Result<LockedRunOutcome, crate::api::VauchiError> {
    network::receive(storage, relay, clock, rng, Some(ohttp))
}

#[cfg(feature = "network-http")]
mod network {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::{LockedReceipt, LockedRunOutcome, store_locked_update};
    use crate::api::relay_transport::{
        TransportOhttpKey, gateway_still_holds, http_relay_url, relay_transport,
    };
    use crate::api::vauchi::RegistrationDay;
    use crate::api::{RelayConfig, VauchiError};
    use crate::clock::Clock;
    use crate::network::mailbox_context::{MailboxContext, MailboxRoute};
    use crate::network::mailbox_token::current_day_epoch;
    use crate::network::request_pacer::RequestPacer;
    use crate::network::{
        AckStatus, Acknowledgment, DelayWindowAdvert, HttpTransportAdapter, MessagePayload,
        OhttpClient, RegisterMailbox, Transport, TransportConfig, create_envelope,
    };
    use crate::rng::{SecureRng, SecureRngExt};
    use crate::storage::LockedReceiveStorage;

    /// The same bound as an unlocked receive: a page that acknowledges
    /// nothing cannot loop forever.
    const MAX_RECEIVE_PAGES: usize = 64;

    pub(super) fn receive(
        storage: &LockedReceiveStorage,
        relay: &RelayConfig,
        clock: Arc<dyn Clock>,
        rng: &dyn SecureRng,
        unanchored: Option<OhttpClient>,
    ) -> Result<LockedRunOutcome, VauchiError> {
        let now = clock.unix_seconds();
        let today = current_day_epoch(now);
        let Some(today_registration) = stored_day(storage, today)? else {
            return Ok(LockedRunOutcome::NothingStored);
        };
        if now >= today_registration.receive_until() {
            return Ok(LockedRunOutcome::WindowEnded);
        }
        if relay.ohttp_delayed
            || storage
                .ohttp_cache()
                .load_delay_window(&relay.ohttp_endpoint())?
                .is_some()
        {
            return Ok(LockedRunOutcome::DelayedFetchesOffered);
        }
        let key = match (relay.ohttp_trust_anchor(), unanchored) {
            (Some(_), _) => held_key(storage, relay, now)?,
            (None, Some(client)) => TransportOhttpKey::Unanchored(client),
            (None, None) => TransportOhttpKey::None,
        };
        if matches!(key, TransportOhttpKey::None) {
            return Ok(LockedRunOutcome::NoOhttpKey);
        }

        let mut days = vec![today_registration];
        days.extend(stored_day(storage, today.saturating_sub(1))?);
        let mut routes = HashMap::new();
        for day in &days {
            routes.extend(
                day.routes()
                    .iter()
                    .map(|(token, route)| (token.clone(), route.clone())),
            );
        }

        let transport = relay_transport(
            relay,
            &relay.server_url,
            relay.connect_timeout_ms,
            Arc::new(RequestPacer::ohttp()),
            key,
            clock.clone(),
        );
        let mut adapter = HttpTransportAdapter::new(transport);
        adapter
            .connect(&TransportConfig::default())
            .map_err(VauchiError::Network)?;
        adapter.set_mailbox_context(Arc::new(MailboxContext {
            identity: [0; 32],
            device: [0; 32],
            application_relay: relay.server_url.clone(),
            ohttp_relay: relay.ohttp_endpoint(),
            routes: routes.clone(),
        }));
        for day in days.iter().rev() {
            for batch in day.batches() {
                let mut tokens = batch.clone();
                rng.shuffle(&mut tokens);
                let envelope = create_envelope(
                    MessagePayload::RegisterMailbox(RegisterMailbox { tokens }),
                    clock.unix_seconds(),
                    rng.uuid_v4().into(),
                );
                adapter.send(&envelope).map_err(VauchiError::Network)?;
            }
        }

        let (mut stored, mut left) = (0, 0);
        for _ in 0..MAX_RECEIVE_PAGES {
            let mut fetched = 0;
            let mut acks = Vec::new();
            while let Some(envelope) = adapter.receive().map_err(VauchiError::Network)? {
                let MessagePayload::EncryptedUpdate(update) = envelope.payload else {
                    continue;
                };
                fetched += 1;
                let receipt = match routes.get(update.recipient_id.as_str()) {
                    Some(MailboxRoute::Contact { id, .. }) => {
                        store_locked_update(storage, id, &update.ciphertext, clock.unix_seconds())?
                    }
                    // Device-sync tokens derive from the master seed; the
                    // unlock receives them (ADR-033 amendment point 3).
                    Some(MailboxRoute::DeviceSync { .. }) | None => LockedReceipt::LeftForUnlock,
                };
                match receipt {
                    LockedReceipt::Stored => {
                        stored += 1;
                        acks.push(envelope.message_id);
                    }
                    LockedReceipt::LeftForUnlock => left += 1,
                }
            }
            for message_id in acks {
                let ack = create_envelope(
                    MessagePayload::Acknowledgment(Acknowledgment {
                        message_id,
                        status: AckStatus::ReceivedByRecipient,
                        error: None,
                    }),
                    clock.unix_seconds(),
                    rng.uuid_v4().into(),
                );
                // A lost ack leaves the blob on the relay; the session
                // already moved past it, so the unlock acknowledges it as
                // one it cannot open.
                if let Err(e) = adapter.send(&ack) {
                    tracing::warn!(target: "vauchi.locked_receive", error = %e, "ack not sent");
                }
            }
            if fetched == 0 || !adapter.last_fetch_truncated() {
                break;
            }
            adapter.allow_refetch();
        }
        // The adapter sends queued acks on its next receive; nothing is
        // fetched again, the last page was drained.
        if let Err(e) = adapter.receive() {
            tracing::warn!(target: "vauchi.locked_receive", error = %e, "acks not sent");
        }
        keep_delay_window(storage, relay, adapter.delay_window_advert());
        Ok(LockedRunOutcome::Received { stored, left })
    }

    /// A relay may start delaying after the last unlocked sync; the run
    /// learns it only from an answer, so it keeps the window for the next
    /// locked run to skip (Socratic discussion 2026-11-09).
    fn keep_delay_window(
        storage: &LockedReceiveStorage,
        relay: &RelayConfig,
        advert: Option<DelayWindowAdvert>,
    ) {
        let Some(DelayWindowAdvert::Advertised(window)) = advert else {
            return;
        };
        if let Err(e) = storage
            .ohttp_cache()
            .save_delay_window(&relay.ohttp_endpoint(), window)
        {
            tracing::warn!(target: "vauchi.locked_receive", error = %e, "delay window not kept");
        }
    }

    fn stored_day(
        storage: &LockedReceiveStorage,
        day: u64,
    ) -> Result<Option<RegistrationDay>, VauchiError> {
        let Some(bytes) = storage.receive_tokens().load_day(day)? else {
            return Ok(None);
        };
        Ok(Some(RegistrationDay::decode(&bytes)?))
    }

    fn held_key(
        storage: &LockedReceiveStorage,
        relay: &RelayConfig,
        now: u64,
    ) -> Result<TransportOhttpKey, VauchiError> {
        let current = vauchi_protocol::ohttp_key::window_of(now);
        Ok(storage
            .ohttp_cache()
            .load_held_ohttp_key(&http_relay_url(relay))?
            .filter(|held| gateway_still_holds(held.window, current))
            .map_or(TransportOhttpKey::None, TransportOhttpKey::Signed))
    }
}

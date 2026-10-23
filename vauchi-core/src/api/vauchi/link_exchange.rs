// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode exchange as a domain API (private#670): share a link or accept
//! one, then advance until the exchange completes. Lets a shell without the
//! presentation protocol (the CLI) run the same ceremony the apps run, over
//! the same core pieces: the initiator and responder state machines, the
//! relay escrow round trip ([`Vauchi::run_escrow_command`]) and
//! [`Vauchi::complete_link_exchange`].

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::card::ContactCard;
use crate::exchange::X3DHKeyPair;
use crate::exchange::link_initiator::{
    LinkInitiatorFailureReason, LinkInitiatorSession, LinkInitiatorState,
};
use crate::exchange::link_mode::{
    initiator_generate_named, parse_exchange_deep_link, responder_respond_with_card_bytes,
    serialize_card_payload_v2,
};
use crate::exchange::link_responder::{
    LinkResponderFailureReason, LinkResponderSession, LinkResponderState,
};
use crate::platform::{Command, Event};

/// How long either side waits for the other, as the apps do (ADR-035's
/// five-minute link window).
const LINK_EXCHANGE_DEADLINE_SECS: u64 = 300;

/// A link-mode exchange in progress on this device.
pub struct LinkExchange {
    role: Role,
    /// Fresh per exchange: its public half is signed into our bootstrap,
    /// its secret completes the exchange (ADR-050).
    x3dh: X3DHKeyPair,
    finished: Option<LinkExchangeProgress>,
}

enum Role {
    Initiator(LinkInitiatorSession),
    Responder(LinkResponderSession),
}

/// Where a link-mode exchange stands after an advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkExchangeProgress {
    /// Still waiting for the other side or the relay.
    Waiting,
    /// Both cards arrived; the contact exists.
    Completed { contact_id: String },
    /// The exchange ended without a contact (a stable failure id).
    Failed { reason: String },
}

impl LinkExchange {
    /// Still waiting on the other side or the relay.
    pub fn is_live(&self) -> bool {
        self.finished.is_none() && matches!(outcome(&self.role), Outcome::Waiting)
    }

    /// The other side's card is being fetched.
    pub fn is_retrieving(&self) -> bool {
        match &self.role {
            Role::Initiator(session) => {
                matches!(session.current_state(), LinkInitiatorState::Retrieving)
            }
            Role::Responder(session) => {
                matches!(session.current_state(), LinkResponderState::Retrieving)
            }
        }
    }
}

impl Vauchi {
    /// Start a link-mode exchange as the initiator: returns the link to send
    /// and the exchange to advance.
    pub fn start_link_exchange(&self) -> VauchiResult<(String, LinkExchange)> {
        let x3dh = X3DHKeyPair::generate();
        let card_bytes = self.link_card_bytes(x3dh.public_key())?;
        let display_name = self
            .identity()
            .map(|identity| identity.display_name().to_string())
            .unwrap_or_default();
        let (initiation, presence) = initiator_generate_named(&display_name);
        let link = initiation.url.clone();
        let deadline = self.clock().unix_seconds() + LINK_EXCHANGE_DEADLINE_SECS;
        let session = LinkInitiatorSession::new(initiation, presence, card_bytes, deadline);
        Ok((
            link,
            LinkExchange {
                role: Role::Initiator(session),
                x3dh,
                finished: None,
            },
        ))
    }

    /// Accept another person's exchange link as the responder.
    pub fn accept_link_exchange(&self, link: &str) -> VauchiResult<LinkExchange> {
        let payload = parse_exchange_deep_link(link.trim())
            .map_err(|e| VauchiError::InvalidState(format!("not an exchange link: {e}")))?;
        self.accept_link_exchange_payload(&payload)
    }

    /// Accept an already parsed (and consented) exchange link.
    pub fn accept_link_exchange_payload(
        &self,
        payload: &crate::exchange::link_mode::DeepLinkPayload,
    ) -> VauchiResult<LinkExchange> {
        let x3dh = X3DHKeyPair::generate();
        let card_bytes = self.link_card_bytes(x3dh.public_key())?;
        let (keys, deposits) = responder_respond_with_card_bytes(payload.as_parsed(), &card_bytes)
            .map_err(|e| VauchiError::InvalidState(format!("link exchange: {e}")))?;
        let deadline = self.clock().unix_seconds() + LINK_EXCHANGE_DEADLINE_SECS;
        Ok(LinkExchange {
            role: Role::Responder(LinkResponderSession::new(keys, deposits, deadline)),
            x3dh,
            finished: None,
        })
    }

    /// Feed one relay event to the exchange, run what it queues, and report
    /// progress. For events that arrive from outside the advance loop.
    pub fn feed_link_exchange_event(
        &self,
        exchange: &mut LinkExchange,
        event: Event,
    ) -> VauchiResult<LinkExchangeProgress> {
        if let Some(done) = &exchange.finished {
            return Ok(done.clone());
        }
        self.apply_and_follow(&mut exchange.role, event);
        Ok(self.settle(exchange))
    }

    /// Run the exchange's pending relay steps once and report progress. Once
    /// finished, keeps reporting the same outcome.
    pub fn advance_link_exchange(
        &self,
        exchange: &mut LinkExchange,
    ) -> VauchiResult<LinkExchangeProgress> {
        if let Some(done) = &exchange.finished {
            return Ok(done.clone());
        }
        let queued = match &mut exchange.role {
            Role::Initiator(session) => session.drain_pending_commands(),
            Role::Responder(session) => session.drain_pending_commands(),
        };
        for command in queued {
            self.run_and_feed(&mut exchange.role, &command);
        }
        // The machines queue a Check only on a transition; poll the gate
        // they are watching once per advance.
        if let Some(gate_hash) = watched_gate(&exchange.role) {
            self.run_and_feed(
                &mut exchange.role,
                &Command::RelayEscrowCheck {
                    gate_hash,
                    suggested_interval_ms: 0,
                },
            );
        }
        let now = self.clock().unix_seconds();
        match &mut exchange.role {
            Role::Initiator(session) => session.tick(now),
            Role::Responder(session) => session.tick(now),
        }

        Ok(self.settle(exchange))
    }

    /// Report progress; on a terminal state, complete the exchange once and
    /// remember the outcome.
    fn settle(&self, exchange: &mut LinkExchange) -> LinkExchangeProgress {
        let progress = match outcome(&exchange.role) {
            Outcome::Waiting => return LinkExchangeProgress::Waiting,
            Outcome::Finalized(card_bytes) => {
                match self.complete_link_exchange(&card_bytes, &exchange.x3dh) {
                    Ok(contact_id) => LinkExchangeProgress::Completed { contact_id },
                    Err(error) => LinkExchangeProgress::Failed {
                        reason: completion_failure(&error),
                    },
                }
            }
            Outcome::Failed(reason) => LinkExchangeProgress::Failed { reason },
        };
        exchange.finished = Some(progress.clone());
        progress
    }

    /// Our signed v2 bootstrap (ADR-050): card, identity signature over the
    /// exchange key, and our relay routing.
    fn link_card_bytes(&self, x3dh_pubkey: &[u8; 32]) -> VauchiResult<Vec<u8>> {
        let identity = self
            .identity()
            .ok_or_else(|| VauchiError::InvalidState("no identity".into()))?;
        let card = match self.own_card()? {
            Some(card) => card,
            None => ContactCard::new(identity.display_name()),
        };
        Ok(serialize_card_payload_v2(
            identity.signing_public_key(),
            identity.signing_keypair(),
            x3dh_pubkey,
            self.relay_server_url(),
            &card,
        ))
    }

    /// Run one escrow command, feed its event to the machine, and run the
    /// follow-ups it queues. The initiator's handshake-gate blob is the
    /// responder's ephemeral key, which it takes as `LinkOpened`.
    fn run_and_feed(&self, role: &mut Role, command: &Command) {
        if let Some(event) = self.run_escrow_command(command) {
            self.apply_and_follow(role, event);
        }
    }

    /// Apply one event to the machine and run the follow-ups it queues.
    fn apply_and_follow(&self, role: &mut Role, mut event: Event) {
        let follow_ups = match role {
            Role::Initiator(session) => {
                if let Event::RelayEscrowBlobReceived { gate_hash, blob } = &event
                    && *gate_hash == session.handshake_gate_bytes()
                {
                    event = Event::LinkOpened {
                        peer_public_key: blob.clone(),
                    };
                }
                session.apply_hardware_event(event);
                session.drain_pending_commands()
            }
            Role::Responder(session) => {
                session.apply_hardware_event(event);
                session.drain_pending_commands()
            }
        };
        for command in follow_ups {
            self.run_and_feed(role, &command);
        }
    }
}

fn watched_gate(role: &Role) -> Option<Vec<u8>> {
    match role {
        Role::Initiator(session) => match session.current_state() {
            LinkInitiatorState::Polling => Some(session.handshake_gate_bytes()),
            LinkInitiatorState::Retrieving => session.escrow_gate_bytes(),
            _ => None,
        },
        Role::Responder(session) => matches!(session.current_state(), LinkResponderState::Polling)
            .then(|| session.gate_hash_bytes()),
    }
}

enum Outcome {
    Waiting,
    Finalized(Vec<u8>),
    Failed(String),
}

fn outcome(role: &Role) -> Outcome {
    match role {
        Role::Initiator(session) => match session.current_state() {
            LinkInitiatorState::Finalized { card_bytes } => Outcome::Finalized(card_bytes.clone()),
            LinkInitiatorState::Failed(reason) => Outcome::Failed(initiator_failure(reason)),
            _ => Outcome::Waiting,
        },
        Role::Responder(session) => match session.current_state() {
            LinkResponderState::Finalized { card_bytes } => Outcome::Finalized(card_bytes.clone()),
            LinkResponderState::Failed(reason) => Outcome::Failed(responder_failure(reason)),
            _ => Outcome::Waiting,
        },
    }
}

fn initiator_failure(reason: &LinkInitiatorFailureReason) -> String {
    match reason {
        LinkInitiatorFailureReason::PollingTimedOut => "polling_timed_out",
        LinkInitiatorFailureReason::HandshakeFailed { .. } => "handshake_failed",
        LinkInitiatorFailureReason::DecryptError { .. } => "decrypt_error",
        LinkInitiatorFailureReason::DepositRejected => "deposit_rejected",
        LinkInitiatorFailureReason::Cancelled => "cancelled",
    }
    .to_string()
}

fn responder_failure(reason: &LinkResponderFailureReason) -> String {
    match reason {
        LinkResponderFailureReason::PollingTimedOut => "polling_timed_out",
        LinkResponderFailureReason::DepositRejected => "deposit_rejected",
        LinkResponderFailureReason::DecryptError { .. } => "decrypt_error",
        LinkResponderFailureReason::Cancelled => "cancelled",
    }
    .to_string()
}

fn completion_failure(error: &VauchiError) -> String {
    match error {
        VauchiError::Exchange(crate::exchange::ExchangeError::SelfExchange) => "self_exchange",
        VauchiError::ContactLimitReached(_) => "contact_limit",
        _ => "completion_failed",
    }
    .to_string()
}

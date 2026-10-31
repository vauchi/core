// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
    LinkExchangeBinding, initiator_generate_named, parse_exchange_deep_link,
    responder_respond_with_signed_card, serialize_link_card_payload,
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
    /// Our card as it was when the exchange started; the initiator signs it
    /// only once the responder's epk is known (#942).
    card: ContactCard,
    /// The responder's binding, known from the start; the initiator's comes
    /// from its session.
    responder_binding: Option<LinkExchangeBinding>,
    finished: Option<LinkExchangeProgress>,
}

enum Role {
    Initiator(Box<LinkInitiatorSession>),
    Responder(Box<LinkResponderSession>),
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
        let card = self.link_card()?;
        let display_name = self
            .identity()
            .map(|identity| identity.display_name().to_string())
            .unwrap_or_default();
        let (initiation, presence) = initiator_generate_named(&display_name);
        let link = initiation.url.clone();
        let deadline = self.clock().unix_seconds() + LINK_EXCHANGE_DEADLINE_SECS;
        let session = LinkInitiatorSession::new(initiation, presence, deadline);
        Ok((
            link,
            LinkExchange {
                role: Role::Initiator(Box::new(session)),
                x3dh,
                card,
                responder_binding: None,
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
        let card = self.link_card()?;
        let identity = self
            .identity()
            .ok_or_else(|| VauchiError::InvalidState("no identity".into()))?;
        let response = responder_respond_with_signed_card(payload.as_parsed(), |binding| {
            serialize_link_card_payload(
                identity.signing_keypair(),
                x3dh.public_key(),
                self.relay_server_url(),
                &card,
                binding,
            )
        })
        .map_err(|e| VauchiError::InvalidState(format!("link exchange: {e}")))?;
        let deadline = self.clock().unix_seconds() + LINK_EXCHANGE_DEADLINE_SECS;
        Ok(LinkExchange {
            role: Role::Responder(Box::new(LinkResponderSession::new(
                response.keys,
                response.handshake_gate,
                response.commands,
                deadline,
            ))),
            x3dh,
            card,
            responder_binding: Some(response.binding),
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
        self.apply_and_follow(exchange, event);
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
            // A failed deposit ends the exchange; the rest of the batch
            // (our encrypted card) must not reach the relay after it.
            if !matches!(outcome(&exchange.role), Outcome::Waiting) {
                break;
            }
            self.run_and_feed(exchange, &command);
        }
        // The machines queue a Check only on a transition; poll the gate
        // they are watching once per advance.
        if let Some(gate_hash) = watched_gate(&exchange.role) {
            self.run_and_feed(
                exchange,
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
            Outcome::Finalized(card_bytes) => match binding(exchange) {
                Some(binding) => {
                    match self.complete_link_exchange(&card_bytes, &exchange.x3dh, &binding) {
                        Ok(contact_id) => LinkExchangeProgress::Completed { contact_id },
                        Err(error) => LinkExchangeProgress::Failed {
                            reason: completion_failure(&error),
                        },
                    }
                }
                None => LinkExchangeProgress::Failed {
                    reason: "completion_failed".to_string(),
                },
            },
            Outcome::Failed(reason) => LinkExchangeProgress::Failed { reason },
        };
        exchange.finished = Some(progress.clone());
        progress
    }

    /// The card our bootstrap carries: our own card, or a bare one with our
    /// display name.
    fn link_card(&self) -> VauchiResult<ContactCard> {
        let identity = self
            .identity()
            .ok_or_else(|| VauchiError::InvalidState("no identity".into()))?;
        Ok(match self.own_card()? {
            Some(card) => card,
            None => ContactCard::new(identity.display_name()),
        })
    }

    /// Run one escrow command, feed its event to the machine, and run the
    /// follow-ups it queues. The initiator's handshake-gate blob is the
    /// responder's ephemeral key, which it takes as `LinkOpened`.
    fn run_and_feed(&self, exchange: &mut LinkExchange, command: &Command) {
        if let Some(event) = self.run_escrow_command(command) {
            self.apply_and_follow(exchange, event);
        }
    }

    /// Apply one event to the machine and run the follow-ups it queues. An
    /// initiator that has just learned the responder's epk gets our card
    /// signed for the exchange binding (#942).
    fn apply_and_follow(&self, exchange: &mut LinkExchange, mut event: Event) {
        let follow_ups = match &mut exchange.role {
            Role::Initiator(session) => {
                if let Event::RelayEscrowBlobReceived { gate_hash, blob } = &event
                    && *gate_hash == session.handshake_gate_bytes()
                {
                    event = Event::LinkOpened {
                        peer_public_key: blob.clone(),
                    };
                }
                session.apply_hardware_event(event);
                if let Some(binding) = session.binding().copied()
                    && matches!(session.current_state(), LinkInitiatorState::AwaitingCard)
                {
                    match self.identity() {
                        Some(identity) => session.deposit_card(&serialize_link_card_payload(
                            identity.signing_keypair(),
                            exchange.x3dh.public_key(),
                            self.relay_server_url(),
                            &exchange.card,
                            &binding,
                        )),
                        // The identity was removed mid-exchange: nothing can
                        // sign our card, so the exchange ends here.
                        None => session.cancel(),
                    }
                }
                session.drain_pending_commands()
            }
            Role::Responder(session) => {
                session.apply_hardware_event(event);
                session.drain_pending_commands()
            }
        };
        for command in follow_ups {
            self.run_and_feed(exchange, &command);
        }
    }
}

/// The exchange both cards are signed for, once it is known.
fn binding(exchange: &LinkExchange) -> Option<LinkExchangeBinding> {
    match &exchange.role {
        Role::Initiator(session) => session.binding().copied(),
        Role::Responder(_) => exchange.responder_binding,
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
        LinkResponderFailureReason::LinkAlreadyUsed => "link_already_used",
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

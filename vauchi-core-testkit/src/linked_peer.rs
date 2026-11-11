// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Two exchanged contacts with stored ratchets: the peer is the ratchet
//! initiator (sends first), the host the responder.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{SymmetricKey, Vauchi};

/// Exchanges `host` and `peer` (exchange time 0) and stores both ratchets.
pub fn link_contacts(host: &Vauchi, peer: &Vauchi, label: &str) -> LinkedPeer {
    let host_pk = *host.identity().unwrap().signing_public_key();
    let peer_pk = *peer.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();

    let peer_at_host =
        Contact::from_exchange(peer_pk, ContactCard::new(label), shared_secret.clone(), 0);
    let peer_contact_id = peer_at_host.id().to_string();
    host.add_contact(peer_at_host).unwrap();

    let host_at_peer =
        Contact::from_exchange(host_pk, ContactCard::new("host"), shared_secret.clone(), 0);
    let host_contact_id = host_at_peer.id().to_string();
    peer.add_contact(host_at_peer).unwrap();

    let host_dh = X3DHKeyPair::generate();
    let peer_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *host_dh.public_key()).unwrap();
    let host_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, host_dh);

    host.storage()
        .ratchets()
        .save_ratchet_state(&peer_contact_id, &host_ratchet, false)
        .unwrap();
    peer.storage()
        .ratchets()
        .save_ratchet_state(&host_contact_id, &peer_ratchet, true)
        .unwrap();

    LinkedPeer {
        peer_contact_id,
        host_contact_id,
        shared_secret,
    }
}

pub struct LinkedPeer {
    /// The peer's contact id in the host's storage.
    pub peer_contact_id: String,
    /// The host's contact id in the peer's storage.
    pub host_contact_id: String,
    pub shared_secret: SymmetricKey,
}

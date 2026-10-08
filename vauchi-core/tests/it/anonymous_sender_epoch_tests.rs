// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Anonymous sender ids across the daily epoch boundary: the previous
//! epoch's id still resolves, and a sender index knows its own epoch
//! (vauchi/private#522).

use vauchi_core::network::anonymous::{SenderIndex, compute_anonymous_id, current_epoch};
use vauchi_core::{Contact, ContactCard, SymmetricKey};

// Senders roll their anonymous id daily; one from the previous epoch, legacy
// or device-scoped, still resolves, and nothing reaches before epoch 0.
// @internal
#[test]
fn a_sender_id_from_the_previous_epoch_still_resolves() {
    use vauchi_core::network::anonymous::{compute_anonymous_id_for_device, resolve_sender_device};

    let shared = SymmetricKey::generate();
    let contacts = vec![Contact::from_exchange(
        [0x33; 32],
        ContactCard::new("Peer"),
        shared.clone(),
        0,
    )];
    let device = [0x44u8; 32];
    let resolved = |id: &[u8; 32], epoch| {
        resolve_sender_device(&contacts, &[device], id, epoch).map(|(_, device_id)| device_id)
    };

    let device_id_yesterday = compute_anonymous_id_for_device(shared.as_bytes(), 4, &device);
    let legacy_yesterday = compute_anonymous_id(shared.as_bytes(), 4);
    assert_eq!(resolved(&device_id_yesterday, 5), Some(device));
    assert_eq!(resolved(&legacy_yesterday, 5), Some([0; 32]));
    assert_eq!(resolved(&[0xEE; 32], 0), None);
    assert_eq!(
        resolved(&compute_anonymous_id(shared.as_bytes(), 0), 0),
        Some([0; 32])
    );
}

// @internal
#[test]
fn an_index_built_for_this_epoch_is_not_stale() {
    let now = 1_700_000_000;
    let index = SenderIndex::build(&[], current_epoch(now));

    assert!(!index.is_stale(now));
}

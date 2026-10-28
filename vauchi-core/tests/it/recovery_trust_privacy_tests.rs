// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whom I trust for recovery stays on my devices: marking a contact
//! sends that contact nothing.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;

// @scenario: contact_recovery :: Trust state is private to the user
#[test]
fn marking_bob_recovery_trusted_queues_nothing_for_bob() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let linked = link_contacts(&bob, &alice, "Alice");
    let bob_id = linked.host_contact_id.clone();
    alice.verify_contact_fingerprint(&bob_id).unwrap();
    let queued_for_bob = || {
        alice
            .storage()
            .pending()
            .get_pending_updates(&bob_id)
            .unwrap()
            .len()
    };
    let queued_before = queued_for_bob();

    alice.toggle_recovery_trust(&bob_id).unwrap();
    alice.run_owed_repropagation().unwrap();

    assert!(
        alice
            .get_contact(&bob_id)
            .unwrap()
            .unwrap()
            .is_recovery_trusted()
    );
    assert_eq!(queued_for_bob(), queued_before);
}

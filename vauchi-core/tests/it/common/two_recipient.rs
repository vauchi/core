// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Two-recipient card-delivery harness.
//!
//! Pairs one or more recipient `Vauchi` instances with a single sharer over
//! real ratchets and the real receive pipeline, so a test can assert what each
//! recipient's stored copy of the sharer's card ends up holding. The sharer is
//! the ratchet initiator; each recipient is the responder.

use std::collections::HashSet;
use vauchi_core::{
    Contact, ContactCard, SymmetricKey, Vauchi, api::process_single_card_update,
    crypto::cek::ContentEncryptionKey, exchange::X3DHKeyPair,
};

/// A recipient instance plus the contact ids that bridge it to the sharer.
pub struct Recipient {
    wb: Vauchi,
    /// The recipient's contact id *in the sharer's* storage (hex of the
    /// recipient's signing key) — what the sharer repropagates to.
    pub id_at_sharer: String,
    /// The sharer's contact id *in the recipient's* storage (hex of the
    /// sharer's signing key) — the card that receives updates.
    sharer_id_here: String,
    /// Pending-update ids already delivered, so each `deliver` only feeds the
    /// recipient's ratchet the newest message (keeps decrypt order trivial).
    delivered: HashSet<String>,
}

/// Pairs a fresh recipient with `sharer`: mutual contacts carrying each
/// other's signing key, a CEK on the sharer side (so `repropagate` takes the
/// CEK-wrapped path the receiver requires), and an initiator/responder
/// ratchet pair over a shared secret.
pub fn add_recipient(sharer: &Vauchi, sharer_pk: &[u8; 32], name: &str) -> Recipient {
    add_recipient_impl(sharer, sharer_pk, name, true)
}

/// Like [`add_recipient`] but leaves the sharer-side contact **without a CEK**,
/// matching a freshly-exchanged contact (`Contact::from_exchange` sets
/// `cek: None`). This exercises the device first-send path the default
/// `add_recipient` masks by pre-seeding a CEK
/// (2026-06-29-card-update-duplicate-message-paths: CEK-less first send).
pub fn add_recipient_no_cek(sharer: &Vauchi, sharer_pk: &[u8; 32], name: &str) -> Recipient {
    add_recipient_impl(sharer, sharer_pk, name, false)
}

fn add_recipient_impl(
    sharer: &Vauchi,
    sharer_pk: &[u8; 32],
    name: &str,
    with_cek: bool,
) -> Recipient {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    let recipient_pk = *wb.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();

    let mut at_sharer =
        Contact::from_exchange(recipient_pk, ContactCard::new(name), shared.clone(), 0);
    if with_cek {
        at_sharer.set_cek(ContentEncryptionKey::generate());
    }
    let id_at_sharer = at_sharer.id().to_string();
    sharer.add_contact(at_sharer).unwrap();

    let sharer_here =
        Contact::from_exchange(*sharer_pk, ContactCard::new("Sharer"), shared.clone(), 0);
    let sharer_id_here = sharer_here.id().to_string();
    wb.add_contact(sharer_here).unwrap();

    let recipient_dh = X3DHKeyPair::generate();
    sharer
        .create_ratchet_as_initiator(&id_at_sharer, &shared, *recipient_dh.public_key())
        .unwrap();
    wb.create_ratchet_as_responder(&sharer_id_here, &shared, recipient_dh)
        .unwrap();

    Recipient {
        wb,
        id_at_sharer,
        sharer_id_here,
        delivered: HashSet::new(),
    }
}

/// Delivers every not-yet-seen pending update from `sharer` to `r`, applying
/// each through the real receive pipeline. Returns how many were delivered.
pub fn deliver(sharer: &Vauchi, r: &mut Recipient) -> usize {
    let pending = sharer
        .storage()
        .pending()
        .get_pending_updates(&r.id_at_sharer)
        .unwrap();
    let mut count = 0;
    for upd in pending {
        if !r.delivered.insert(upd.id.clone()) {
            continue;
        }
        process_single_card_update(
            r.wb.identity().unwrap(),
            r.wb.storage(),
            &r.sharer_id_here,
            &upd.payload,
        )
        .unwrap_or_else(|e| panic!("delivery failed: {e:?}"));
        count += 1;
    }
    count
}

/// The display name on the recipient's stored copy of the sharer's card.
pub fn stored_card_display_name(r: &Recipient) -> String {
    r.wb.storage()
        .contacts()
        .load_contact(&r.sharer_id_here)
        .unwrap()
        .unwrap()
        .card()
        .display_name()
        .to_string()
}

/// Whether the recipient's stored copy of the sharer's card holds a field.
pub fn stored_card_has(r: &Recipient, label: &str) -> bool {
    r.wb.storage()
        .contacts()
        .load_contact(&r.sharer_id_here)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == label)
}

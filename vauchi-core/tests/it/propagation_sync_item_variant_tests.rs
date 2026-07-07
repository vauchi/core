// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Coverage for the `apply_sync_items` SyncItem-variant arms in
//! `api/vauchi/propagation.rs` that previous tests didn't reach:
//! `DeletionScheduled`, `DeletionCancelled`, `ImportedContactRemoved`,
//! and the visibility-changed/personal-note/proposal-trust paths.

use std::sync::{Arc, Mutex};

use vauchi_core::Vauchi;
use vauchi_core::api::VauchiEvent;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::DeletionState;
use vauchi_core::sync::{ImportedContactSyncData, SyncItem};

fn make_vauchi() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn make_exchanged_contact(name: &str) -> Contact {
    let identity = Identity::create(name, 0);
    Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ============================================================
// DeletionScheduled / DeletionCancelled
// ============================================================

// @scenario: privacy_compliance :: Identity deletion propagates across all user devices
// @internal
#[test]
fn apply_sync_deletion_scheduled_writes_state_to_storage() {
    let wb = make_vauchi();
    let scheduled_at = now();
    let execute_at = scheduled_at + 7 * 24 * 60 * 60;

    let applied = wb
        .apply_sync_items(vec![SyncItem::DeletionScheduled {
            scheduled_at,
            execute_at,
            timestamp: scheduled_at,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    let state = wb.storage().consent().load_deletion_state().unwrap();
    match state {
        DeletionState::Scheduled {
            scheduled_at: got_scheduled,
            execute_at: got_execute,
        } => {
            assert_eq!(got_scheduled, scheduled_at);
            assert_eq!(got_execute, execute_at);
        }
        other => panic!("expected DeletionState::Scheduled, got {other:?}"),
    }
}

// @internal
#[test]
fn apply_sync_deletion_cancelled_clears_state() {
    let wb = make_vauchi();
    wb.storage()
        .consent()
        .save_deletion_state(&DeletionState::Scheduled {
            scheduled_at: 100,
            execute_at: 200,
        })
        .unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::DeletionCancelled { timestamp: now() }])
        .unwrap();

    assert_eq!(applied, 1);
    let state = wb.storage().consent().load_deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::None),
        "DeletionCancelled must clear scheduled state to None, got {state:?}"
    );
}

// ============================================================
// ============================================================

// @internal
#[test]
fn apply_sync_imported_contact_removed_deletes_contact() {
    let wb = make_vauchi();

    // Add an imported contact directly via storage so we have something
    // to remove. Use a UUID-style id matching the imported-contact convention.
    let imported_card = ContactCard::new("Charlie");
    let imported_contact = Contact::from_import(
        "contact-charlie".to_string(),
        imported_card,
        vauchi_core::contact::ImportSource::Manual,
        None,
        0,
    );
    let imported_id = imported_contact.id().to_string();
    wb.add_contact(imported_contact).unwrap();
    assert!(
        wb.get_contact(&imported_id).unwrap().is_some(),
        "precondition: imported contact must exist before sync removal"
    );

    let applied = wb
        .apply_sync_items(vec![SyncItem::ImportedContactRemoved {
            contact_id: imported_id.to_string(),
            timestamp: now(),
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(
        wb.get_contact(&imported_id).unwrap().is_none(),
        "ImportedContactRemoved must delete the contact from storage"
    );
}

// @internal
#[test]
fn apply_sync_imported_contact_removed_nonexistent_is_skipped_non_fatally() {
    let wb = make_vauchi();

    let applied = wb
        .apply_sync_items(vec![SyncItem::ImportedContactRemoved {
            contact_id: "no-such-imported-id".to_string(),
            timestamp: now(),
        }])
        .unwrap();

    // The arm uses storage.delete_contact which is idempotent; counted as applied.
    assert_eq!(applied, 1);
}

// ============================================================
// ============================================================

// @internal
#[test]
fn apply_sync_personal_note_changed_persists_note() {
    let wb = make_vauchi();
    let bob = make_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::PersonalNoteChanged {
            contact_id: bob_id.clone(),
            note: "met at conference".to_string(),
            timestamp: now(),
        }])
        .unwrap();

    assert_eq!(applied, 1);
    let note_bytes = wb
        .storage()
        .contacts()
        .load_personal_notes(&bob_id)
        .unwrap();
    let note = String::from_utf8(note_bytes.unwrap_or_default()).unwrap();
    assert_eq!(note, "met at conference");
}

// ============================================================
// ============================================================

// @internal
#[test]
fn apply_sync_proposal_trust_changed_updates_contact_flag() {
    let wb = make_vauchi();
    let bob = make_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    assert!(
        !wb.get_contact(&bob_id)
            .unwrap()
            .unwrap()
            .is_proposal_trusted(),
        "default must be not proposal-trusted"
    );

    let applied = wb
        .apply_sync_items(vec![SyncItem::ProposalTrustChanged {
            contact_id: bob_id.clone(),
            proposal_trusted: true,
            timestamp: now(),
        }])
        .unwrap();
    assert_eq!(applied, 1);

    let bob_after = wb.get_contact(&bob_id).unwrap().unwrap();
    assert!(bob_after.is_proposal_trusted(), "flag must flip to true");

    // Roundtrip: flip back to false.
    let _ = wb
        .apply_sync_items(vec![SyncItem::ProposalTrustChanged {
            contact_id: bob_id.clone(),
            proposal_trusted: false,
            timestamp: now(),
        }])
        .unwrap();
    assert!(
        !wb.get_contact(&bob_id)
            .unwrap()
            .unwrap()
            .is_proposal_trusted(),
        "flag must flip back to false"
    );
}

// @internal
#[test]
fn apply_sync_proposal_trust_changed_for_unknown_contact_is_skipped() {
    let wb = make_vauchi();
    let applied = wb
        .apply_sync_items(vec![SyncItem::ProposalTrustChanged {
            contact_id: "no-such-id".to_string(),
            proposal_trusted: true,
            timestamp: now(),
        }])
        .unwrap();
    assert_eq!(applied, 1, "unknown contact is a non-fatal skip");
}

// ============================================================
// LabelChange — re-apply visible_fields branch (line 624-629)
// ============================================================

// @internal
#[test]
fn apply_sync_label_change_creates_then_updates_with_fields() {
    let wb = make_vauchi();
    let bob = make_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let create = vec![SyncItem::LabelChange {
        label_id: "label-friends".to_string(),
        label_name: "Friends".to_string(),
        contacts: vec![],
        visible_fields: vec![],
        is_deleted: false,
        timestamp: now(),
    }];
    wb.apply_sync_items(create).unwrap();

    // Now update the label with contacts AND visible_fields — this
    // exercises the "re-apply field visibility" loop (line 624-629).
    let update = vec![SyncItem::LabelChange {
        label_id: "label-friends".to_string(),
        label_name: "Close Friends".to_string(),
        contacts: vec![bob_id.clone()],
        visible_fields: vec!["email".to_string(), "phone".to_string()],
        is_deleted: false,
        timestamp: now(),
    }];
    let applied = wb.apply_sync_items(update).unwrap();
    assert_eq!(applied, 1);
}

// @internal
#[test]
fn apply_sync_label_change_with_is_deleted_removes_label() {
    let wb = make_vauchi();
    // The LabelChange (is_deleted=false) arm creates the label via
    // create_group(label_name), which assigns its own internal ID
    // (UUID). To exercise the is_deleted branch we need the real ID.
    // First create directly so we know the ID.
    let label = wb.storage().labels().create_group("ToBeDeleted").unwrap();
    let real_label_id = label.id().to_string();
    assert!(
        wb.storage().labels().load_group(&real_label_id).is_ok(),
        "precondition"
    );

    let applied = wb
        .apply_sync_items(vec![SyncItem::LabelChange {
            label_id: real_label_id.clone(),
            label_name: String::new(),
            contacts: vec![],
            visible_fields: vec![],
            is_deleted: true,
            timestamp: now(),
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert!(
        wb.storage().labels().load_group(&real_label_id).is_err(),
        "label must be deleted from storage"
    );
}

// ============================================================
// ============================================================

// @internal
#[test]
fn apply_sync_visibility_changed_writes_per_contact_override() {
    let wb = make_vauchi();
    let bob = make_exchanged_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::VisibilityChanged {
            contact_id: bob_id.clone(),
            field_id: "email".to_string(),
            is_visible: false,
            timestamp: now(),
        }])
        .unwrap();
    assert_eq!(applied, 1);
}

// ============================================================
// ImportedContact roundtrip — Add → Update → Remove
// ============================================================

// @internal
#[test]
fn apply_sync_imported_contact_add_update_remove_roundtrip() {
    let wb = make_vauchi();
    let card = ContactCard::new("Dora");
    let imported_contact = Contact::from_import(
        "contact-dora".to_string(),
        card,
        vauchi_core::contact::ImportSource::Manual,
        None,
        0,
    );
    let imported_id = imported_contact.id().to_string();
    let sync_data = ImportedContactSyncData::from_contact(&imported_contact).unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::ImportedContactAdded {
            contact_data: sync_data.clone(),
            timestamp: now(),
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert!(wb.get_contact(&imported_id).unwrap().is_some());

    // path: build an updated ImportedContactSyncData with the same id,
    // changed display_name.
    let mut updated_sync_data = sync_data;
    updated_sync_data.display_name = "Dora Updated".to_string();
    let mut updated_card = ContactCard::new("Dora Updated");
    let _ = updated_card.set_display_name("Dora Updated");
    updated_sync_data.card_json = serde_json::to_string(&updated_card).unwrap();

    let applied = wb
        .apply_sync_items(vec![SyncItem::ImportedContactUpdated {
            contact_data: updated_sync_data,
            timestamp: now() + 1,
        }])
        .unwrap();
    assert_eq!(applied, 1);
    let after_update = wb.get_contact(&imported_id).unwrap().unwrap();
    assert_eq!(after_update.display_name(), "Dora Updated");

    let applied = wb
        .apply_sync_items(vec![SyncItem::ImportedContactRemoved {
            contact_id: imported_id.to_string(),
            timestamp: now() + 2,
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert!(wb.get_contact(&imported_id).unwrap().is_none());
}

// ============================================================
// Event dispatch — device-synced changes must live-refresh the UI
//
// Gap A of `2026-06-30-sync-ui-invalidation-sibling-gaps`: every
// `apply_sync_items` arm persisted storage but only `ContactAdded`
// dispatched a `VauchiEvent`, so `affected_screens` never fired and the
// receiving device's screen stayed stale until navigation. Each arm with a
// mapped `affected_screens` entry must now dispatch its event (same class as
// the `!1209` relay-receive fix). Deletion-state arms have no mapped
// invalidation event and must dispatch nothing.
// ============================================================

fn capture_events(wb: &Vauchi) -> Arc<Mutex<Vec<VauchiEvent>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let _ = wb.add_event_handler(Arc::new(move |event| sink.lock().unwrap().push(event)));
    events
}

fn added_contact(wb: &mut Vauchi, name: &str) -> String {
    let contact = make_exchanged_contact(name);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

// @internal
#[test]
fn apply_sync_contact_removed_dispatches_contact_removed_event() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::ContactRemoved {
        contact_id: bob_id.clone(),
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(got.as_slice(), [VauchiEvent::ContactRemoved { contact_id }] if *contact_id == bob_id),
        "ContactRemoved sync must dispatch one ContactRemoved event, got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_card_updated_dispatches_own_card_updated_event() {
    let wb = make_vauchi();
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "a@b.example".to_string(),
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::OwnCardUpdated { changed_fields }] if changed_fields == &["email".to_string()]
        ),
        "CardUpdated sync must dispatch OwnCardUpdated(['email']), got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_visibility_changed_dispatches_visibility_changed_event() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::VisibilityChanged {
        contact_id: bob_id.clone(),
        field_id: "email".to_string(),
        is_visible: false,
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::VisibilityChanged { contact_id, field }]
                if *contact_id == bob_id && field == "email"
        ),
        "VisibilityChanged sync must dispatch VisibilityChanged, got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_label_change_dispatches_label_sync_completed_event() {
    let wb = make_vauchi();
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::LabelChange {
        label_id: "label-friends".to_string(),
        label_name: "Friends".to_string(),
        contacts: vec![],
        visible_fields: vec![],
        is_deleted: false,
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::LabelSyncCompleted { label_id }] if label_id == "label-friends"
        ),
        "LabelChange sync must dispatch LabelSyncCompleted, got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_contact_trust_changed_dispatches_contact_updated_event() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::ContactTrustChanged {
        contact_id: bob_id.clone(),
        recovery_trusted: true,
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::ContactUpdated { contact_id, changed_fields }]
                if *contact_id == bob_id && changed_fields == &["recovery_trusted".to_string()]
        ),
        "ContactTrustChanged sync must dispatch ContactUpdated(['recovery_trusted']), got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_personal_note_changed_dispatches_contact_updated_event() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::PersonalNoteChanged {
        contact_id: bob_id.clone(),
        note: "met at conf".to_string(),
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::ContactUpdated { contact_id, changed_fields }]
                if *contact_id == bob_id && changed_fields == &["personal_note".to_string()]
        ),
        "PersonalNoteChanged sync must dispatch ContactUpdated(['personal_note']), got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_proposal_trust_changed_dispatches_contact_updated_event() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::ProposalTrustChanged {
        contact_id: bob_id.clone(),
        proposal_trusted: true,
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::ContactUpdated { contact_id, changed_fields }]
                if *contact_id == bob_id && changed_fields == &["proposal_trusted".to_string()]
        ),
        "ProposalTrustChanged sync must dispatch ContactUpdated(['proposal_trusted']), got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_contact_archived_then_unarchived_dispatch_matching_events() {
    let mut wb = make_vauchi();
    let bob_id = added_contact(&mut wb, "Bob");

    let archived = capture_events(&wb);
    wb.apply_sync_items(vec![SyncItem::ContactArchived {
        contact_id: bob_id.clone(),
        timestamp: now(),
    }])
    .unwrap();
    assert!(
        matches!(
            archived.lock().unwrap().as_slice(),
            [VauchiEvent::ContactArchived { contact_id }] if *contact_id == bob_id
        ),
        "ContactArchived sync must dispatch ContactArchived"
    );

    let unarchived = capture_events(&wb);
    wb.apply_sync_items(vec![SyncItem::ContactUnarchived {
        contact_id: bob_id.clone(),
        timestamp: now(),
    }])
    .unwrap();
    assert!(
        matches!(
            unarchived.lock().unwrap().as_slice(),
            // Two handlers are now attached (archived's + this one); assert the
            // last handler saw exactly one ContactUnarchived.
            [VauchiEvent::ContactUnarchived { contact_id }] if *contact_id == bob_id
        ),
        "ContactUnarchived sync must dispatch ContactUnarchived"
    );
}

// @internal
#[test]
fn apply_sync_imported_contact_added_dispatches_contact_added_event() {
    let wb = make_vauchi();
    let card = ContactCard::new("Dora");
    let imported = Contact::from_import(
        "contact-dora".to_string(),
        card,
        vauchi_core::contact::ImportSource::Manual,
        None,
        0,
    );
    let imported_id = imported.id().to_string();
    let sync_data = ImportedContactSyncData::from_contact(&imported).unwrap();
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::ImportedContactAdded {
        contact_data: sync_data,
        timestamp: now(),
    }])
    .unwrap();

    let got = events.lock().unwrap();
    assert!(
        matches!(
            got.as_slice(),
            [VauchiEvent::ContactAdded { contact_id, .. }] if *contact_id == imported_id
        ),
        "ImportedContactAdded sync must dispatch ContactAdded, got {got:?}"
    );
}

// @internal
#[test]
fn apply_sync_deletion_scheduled_dispatches_no_invalidation_event() {
    let wb = make_vauchi();
    let events = capture_events(&wb);

    wb.apply_sync_items(vec![SyncItem::DeletionScheduled {
        scheduled_at: now(),
        execute_at: now() + 1,
        timestamp: now(),
    }])
    .unwrap();

    // No `VauchiEvent` maps deletion-state to a screen today; the settings
    // screen still refreshes on next navigation. Documented residual of Gap A.
    assert!(
        events.lock().unwrap().is_empty(),
        "DeletionScheduled has no mapped invalidation event — must dispatch nothing"
    );
}

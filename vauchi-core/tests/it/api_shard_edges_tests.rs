// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small API edges a mutation shard found untested: the pending update
//! count across contacts, the pre-signed error texts and the time a
//! revocation is stamped with (vauchi/private#522).

use super::common::device_sync::create_test_contact;
use super::common::helpers::create_vauchi_with_identity;
use vauchi_core::api::PreSignedError;
use vauchi_core::storage::{PendingUpdate, UpdateStatus};

fn pending(id: &str, contact_id: &str) -> PendingUpdate {
    PendingUpdate {
        id: id.into(),
        contact_id: contact_id.into(),
        update_type: "card_delta".into(),
        payload: vec![1],
        created_at: 0,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    }
}

// @internal
#[test]
fn the_pending_count_adds_up_every_contacts_queue() {
    let wb = create_vauchi_with_identity("Alice");
    let bob = create_test_contact("Bob");
    let carol = create_test_contact("Carol");
    for contact in [&bob, &carol] {
        wb.storage().contacts().save_contact(contact).unwrap();
    }
    for (id, contact) in [("u1", &bob), ("u2", &bob), ("u3", &carol)] {
        wb.storage()
            .pending()
            .queue_update(&pending(id, contact.id()))
            .unwrap();
    }

    assert_eq!(wb.pending_update_count().unwrap(), 3);
}

// @internal
#[test]
fn pre_signed_errors_name_what_failed() {
    assert_eq!(
        PreSignedError::SerializationFailed("x".into()).to_string(),
        "Serialization failed: x"
    );
    assert_eq!(
        PreSignedError::DeserializationFailed("y".into()).to_string(),
        "Deserialization failed: y"
    );
    assert_eq!(
        PreSignedError::IoError("z".into()).to_string(),
        "I/O error: z"
    );
}

// @internal
#[test]
fn a_revocation_is_stamped_with_the_app_clock() {
    use std::time::{Duration, SystemTime};
    use vauchi_core::clock::FakeClock;
    use vauchi_core::identity::DeviceInfo;

    const NOW: u64 = 1_700_000_000;
    let seed = [9u8; 32];
    let clock = FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(NOW)).shared();
    let mut wb = vauchi_core::Vauchi::in_memory_with_clock(clock).unwrap();
    wb.create_identity("Alice").unwrap();
    let mut registry = wb.identity().unwrap().initial_device_registry();
    let laptop = DeviceInfo::derive(&seed, 1, "Laptop".into(), 0).to_registered(&seed);
    let laptop_id = laptop.device_id;
    registry.add_device_unsigned(laptop).unwrap();
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();

    wb.revoke_device(1).unwrap();

    let saved = wb
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    let revoked = saved.find_device(&laptop_id).unwrap();
    assert_eq!(revoked.revoked_at, Some(NOW));
}

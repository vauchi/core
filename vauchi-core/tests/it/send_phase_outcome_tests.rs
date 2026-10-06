// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a send phase reports about its work: retry ticks on reconnect,
//! failed sends per contact, and envelopes sealed for their device
//! (vauchi/private#522).

use super::common::device_sync::{create_test_device, create_test_registry};
use std::sync::Arc;
use vauchi_core::api::*;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::{MockTransport, RelayClientConfig, TransportConfig};
use vauchi_core::storage::{PendingUpdate, UpdateStatus};
use vauchi_core::sync::SyncItem;
use vauchi_core::*;
use vauchi_core::{Contact, ContactCard, ImportSource};

fn create_test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn create_test_relay() -> RelayClient<MockTransport> {
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        ..Default::default()
    };
    RelayClient::new(MockTransport::new(), config, "test-identity".into())
}

fn pending(contact_id: &str, payload: Vec<u8>) -> PendingUpdate {
    PendingUpdate {
        id: "update".into(),
        contact_id: contact_id.to_string(),
        update_type: "card_delta".into(),
        payload,
        created_at: 0,
        retry_count: 0,
        status: UpdateStatus::Pending,
        target_relay_url: None,
        target_device_id: None,
    }
}

fn queue_sendable_update(storage: &Storage, contact_id: &str, shared: &SymmetricKey) {
    queue_sendable_update_as(storage, contact_id, shared, "update");
}

fn queue_sendable_update_as(storage: &Storage, contact_id: &str, shared: &SymmetricKey, id: &str) {
    let peer_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(shared, *peer_dh.public_key()).unwrap();
    let msg = ratchet.encrypt(b"payload").unwrap();
    storage
        .pending()
        .queue_update(&PendingUpdate {
            id: id.into(),
            ..pending(contact_id, serde_json::to_vec(&msg).unwrap())
        })
        .unwrap();
}

/// The retry-tick reports a send phase emits when it connects with these
/// retry entries queued.
fn retry_reports_on_connect(entries: &[(u32, u32)]) -> Vec<String> {
    use std::sync::Mutex;
    use vauchi_core::storage::RetryEntry;

    let storage = create_test_storage();
    for (n, (attempt, max_attempts)) in entries.iter().enumerate() {
        storage
            .retries()
            .create_retry_entry(&RetryEntry {
                message_id: format!("m{n}"),
                recipient_id: "bob".into(),
                payload: vec![1],
                attempt: *attempt,
                next_retry: 0,
                created_at: 0,
                max_attempts: *max_attempts,
            })
            .unwrap();
    }
    let events = Arc::new(EventDispatcher::new());
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&statuses);
    events.on_event(move |event| {
        if let VauchiEvent::DeliveryStatusUpdate { status, .. } = event {
            sink.lock().unwrap().push(status);
        }
    });
    let mut controller =
        SendPhase::new(create_test_relay(), &storage, SyncConfig::default(), events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    statuses.lock().unwrap().clone()
}

// @internal
#[test]
fn reconnecting_reports_retries_only_when_some_ran() {
    assert!(
        retry_reports_on_connect(&[]).is_empty(),
        "nothing due, nothing said"
    );
    assert_eq!(
        retry_reports_on_connect(&[(3, 3)]),
        ["Retry tick: 0 rescheduled, 1 expired"]
    );
    assert_eq!(
        retry_reports_on_connect(&[(0, 3)]),
        ["Retry tick: 1 rescheduled, 0 expired"]
    );
}

fn controller_with_one_contact(
    storage: &Storage,
) -> (SendPhase<'_, MockTransport>, String, SymmetricKey) {
    controller_reporting_to(storage, Arc::new(EventDispatcher::new()))
}

fn controller_reporting_to(
    storage: &Storage,
    events: Arc<EventDispatcher>,
) -> (SendPhase<'_, MockTransport>, String, SymmetricKey) {
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange([0x33u8; 32], ContactCard::new("Peer"), shared.clone(), 0);
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    storage
        .device()
        .save_device_info(&[0x55; 32], 0, "Local", 0)
        .unwrap();
    let peer_dh = X3DHKeyPair::generate();
    let ratchet = DoubleRatchetState::initialize_initiator(&shared, *peer_dh.public_key()).unwrap();
    storage
        .ratchets()
        .save_ratchet_state_for_device(&contact_id, &[0x44; 32], &ratchet, true)
        .unwrap();
    let mut controller =
        SendPhase::new(create_test_relay(), storage, SyncConfig::default(), events);
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    (controller, contact_id, shared)
}

// @internal
#[test]
fn sync_contact_counts_an_undecodable_update_as_failed() {
    let storage = create_test_storage();
    let (mut controller, contact_id, _) = controller_with_one_contact(&storage);
    storage
        .pending()
        .queue_update(&pending(&contact_id, b"not a ratchet message".to_vec()))
        .unwrap();

    let result = controller.sync_contact(&contact_id).unwrap();

    assert_eq!((result.sent, result.failed), (0, 1));
}

// @internal
#[test]
fn sync_contact_counts_a_refused_send_as_failed() {
    let storage = create_test_storage();
    let (mut controller, contact_id, shared) = controller_with_one_contact(&storage);
    queue_sendable_update(&storage, &contact_id, &shared);
    controller
        .relay_mut()
        .connection_mut()
        .transport_mut()
        .inject_error(vauchi_core::network::NetworkError::SendFailed(
            "refused".into(),
        ));

    let result = controller.sync_contact(&contact_id).unwrap();

    assert_eq!((result.sent, result.failed), (0, 1));
}

// @internal
#[test]
fn a_pending_change_is_sealed_for_the_device_it_is_addressed_to() {
    let seed = [0x42u8; 32];
    let phone = create_test_device(&seed, 0, "Phone");
    let laptop = create_test_device(&seed, 1, "Laptop");
    let mut registry = create_test_registry(&seed, &phone);
    registry
        .add_device_unsigned(laptop.to_registered(&seed))
        .unwrap();
    let phone_storage = create_test_storage();
    let mut phone_side = DeviceSyncOrchestrator::new(
        &phone_storage,
        create_test_device(&seed, 0, "Phone"),
        registry.clone(),
    );
    phone_side
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+41 79".to_string(),
            timestamp: 1000,
        })
        .unwrap();
    let identity =
        vauchi_core::Identity::from_device_link(seed, "Alice".into(), 0, "Phone".into(), 0);

    let envelopes = phone_side.build_outbound_envelopes(&identity).unwrap();

    assert_eq!(envelopes.len(), 1);
    let envelope: serde_json::Value = serde_json::from_slice(&envelopes[0]).unwrap();
    let target: Vec<u8> = serde_json::from_value(envelope["target_device_id"].clone()).unwrap();
    assert_eq!(target.as_slice(), laptop.device_id());
    let ciphertext: Vec<u8> = serde_json::from_value(envelope["ciphertext"].clone()).unwrap();
    let laptop_storage = create_test_storage();
    let laptop_side = DeviceSyncOrchestrator::new(&laptop_storage, laptop, registry);
    let plaintext = laptop_side
        .decrypt_from_device(phone.exchange_public_key(), &ciphertext)
        .expect("only the addressed device's key opens it");
    assert!(String::from_utf8_lossy(&plaintext).contains("+41 79"));
}

// @internal
#[test]
fn sync_reports_progress_counting_up_to_the_total() {
    use std::sync::Mutex;
    let storage = create_test_storage();
    let events = Arc::new(EventDispatcher::new());
    let progress = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&progress);
    events.on_event(move |event| {
        if let VauchiEvent::SyncProgress {
            total, processed, ..
        } = event
        {
            sink.lock().unwrap().push((processed, total));
        }
    });
    let (mut controller, contact_id, shared) = controller_reporting_to(&storage, events);
    queue_sendable_update_as(&storage, &contact_id, &shared, "first");
    queue_sendable_update_as(&storage, &contact_id, &shared, "second");

    controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();

    assert_eq!(*progress.lock().unwrap(), [(1, 2), (2, 2)]);
}

// @internal
#[test]
fn a_device_with_a_sibling_builds_envelopes_for_its_pending_changes() {
    let seed = [0x42u8; 32];
    let phone = create_test_device(&seed, 0, "Phone");
    let mut registry = create_test_registry(&seed, &phone);
    let storage = create_test_storage();
    let identity =
        vauchi_core::Identity::from_device_link(seed, "Alice".into(), 0, "Phone".into(), 0);
    let lone = registry.clone();
    storage.device().save_device_registry(&lone).unwrap();
    assert!(
        build_device_sync_envelopes(&identity, &storage)
            .unwrap()
            .is_empty()
    );

    registry
        .add_device_unsigned(create_test_device(&seed, 1, "Laptop").to_registered(&seed))
        .unwrap();
    storage.device().save_device_registry(&registry).unwrap();
    DeviceSyncOrchestrator::new(&storage, phone, registry)
        .record_local_change(SyncItem::CardUpdated {
            field_label: "phone".to_string(),
            new_value: "+41 79".to_string(),
            timestamp: 1000,
        })
        .unwrap();

    assert_eq!(
        build_device_sync_envelopes(&identity, &storage)
            .unwrap()
            .len(),
        1
    );
}

fn sync_once(controller: &mut SendPhase<'_, MockTransport>) -> SyncResult {
    controller
        .sync(&vauchi_core::rng::OsSecureRng::new())
        .unwrap()
}

// @internal
#[test]
fn sync_counts_every_update_it_cannot_send_as_failed() {
    let storage = create_test_storage();
    let (mut controller, contact_id, _) = controller_with_one_contact(&storage);
    let imported = Contact::from_import(
        "imported".into(),
        ContactCard::new("Imported"),
        ImportSource::VcardFile,
        None,
        0,
    );
    storage.contacts().save_contact(&imported).unwrap();
    let queue = |id: &str, contact: &str, payload: &[u8]| {
        storage
            .pending()
            .queue_update(&PendingUpdate {
                id: id.into(),
                ..pending(contact, payload.to_vec())
            })
            .unwrap();
    };
    queue("no-contact", "ghost", b"x");
    queue("no-key", "imported", b"x");
    queue("garbled", &contact_id, b"not a ratchet message");

    let result = sync_once(&mut controller);

    assert_eq!((result.sent, result.failed), (0, 3));
    assert_eq!(result.errors.len(), 3);
}

// @internal
#[test]
fn a_refused_send_is_failed_and_its_retry_count_goes_up() {
    let storage = create_test_storage();
    let (mut controller, contact_id, shared) = controller_with_one_contact(&storage);
    queue_sendable_update(&storage, &contact_id, &shared);
    controller
        .relay_mut()
        .connection_mut()
        .transport_mut()
        .inject_send_error(vauchi_core::network::NetworkError::SendFailed(
            "refused".into(),
        ));

    let result = sync_once(&mut controller);

    assert_eq!((result.sent, result.failed), (0, 1));
    let update = storage
        .pending()
        .get_pending_update("update")
        .unwrap()
        .unwrap();
    assert_eq!(update.retry_count, 1);
}

// @internal
#[test]
fn a_batch_size_caps_the_updates_sent_per_cycle_and_zero_means_no_cap() {
    for (batch_size, expected_sent) in [(Some(1), 1), (Some(0), 2), (None, 2)] {
        let storage = create_test_storage();
        let (_, contact_id, shared) = controller_with_one_contact(&storage);
        queue_sendable_update_as(&storage, &contact_id, &shared, "first");
        queue_sendable_update_as(&storage, &contact_id, &shared, "second");
        let mut controller = SendPhase::new(
            create_test_relay(),
            &storage,
            SyncConfig {
                batch_size,
                ..SyncConfig::default()
            },
            Arc::new(EventDispatcher::new()),
        );
        controller
            .connect(&vauchi_core::rng::OsSecureRng::new())
            .unwrap();

        assert_eq!(
            sync_once(&mut controller).sent,
            expected_sent,
            "{batch_size:?}"
        );
    }
}

fn controller_over(
    storage: &Storage,
    transport: MockTransport,
    ack_timeout_ms: u64,
) -> SendPhase<'_, MockTransport> {
    let relay = RelayClient::new(
        transport,
        RelayClientConfig {
            ack_timeout_ms,
            ..Default::default()
        },
        "test-identity".into(),
    );
    let mut controller = SendPhase::new(
        relay,
        storage,
        SyncConfig::default(),
        Arc::new(EventDispatcher::new()),
    );
    controller
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    controller
}

// @internal
#[test]
fn the_next_sync_counts_the_acknowledgment_of_a_sent_update() {
    let storage = create_test_storage();
    let (_, contact_id, shared) = controller_with_one_contact(&storage);
    queue_sendable_update(&storage, &contact_id, &shared);
    let mut transport = MockTransport::new();
    transport.set_auto_ack(true);
    let mut controller = controller_over(&storage, transport, 30_000);

    let first = sync_once(&mut controller);
    let second = sync_once(&mut controller);

    assert_eq!((first.sent, first.acknowledged), (1, 0));
    assert_eq!((second.acknowledged, second.timed_out), (1, 0));
    assert!(second.errors.is_empty(), "{:?}", second.errors);
}

// @internal
#[test]
fn the_next_sync_counts_an_unacknowledged_send_as_timed_out() {
    let storage = create_test_storage();
    let (_, contact_id, shared) = controller_with_one_contact(&storage);
    queue_sendable_update(&storage, &contact_id, &shared);
    let mut controller = controller_over(&storage, MockTransport::new(), 0);

    assert_eq!(sync_once(&mut controller).sent, 1);
    let second = sync_once(&mut controller);

    assert_eq!((second.acknowledged, second.timed_out), (0, 1));
}

// A registry handshake addressed to a device keeps the anonymous sender token
// but still stamps this device as origin, which needs the local device info.
// @internal
#[test]
fn a_device_routed_handshake_without_local_device_info_fails() {
    let storage = create_test_storage();
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange([0x33u8; 32], ContactCard::new("Peer"), shared.clone(), 0);
    let contact_id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    let peer_dh = X3DHKeyPair::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *peer_dh.public_key()).unwrap();
    let msg = ratchet.encrypt(b"payload").unwrap();
    storage
        .pending()
        .queue_update(&PendingUpdate {
            update_type: vauchi_core::api::sync::REGISTRY_HANDSHAKE_UPDATE_TYPE.into(),
            target_device_id: Some([0x44; 32]),
            ..pending(&contact_id, serde_json::to_vec(&msg).unwrap())
        })
        .unwrap();
    let mut controller = controller_over(&storage, MockTransport::new(), 30_000);

    let result = sync_once(&mut controller);

    assert_eq!((result.sent, result.failed), (0, 1));
}

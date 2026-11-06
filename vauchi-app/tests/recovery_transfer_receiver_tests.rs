// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::orchestrator::recovery_receiver::{ReceiveState, RecoveryTransferReceiver};
use vauchi_core::backup::{
    BackupKey, GuardianBackupMetadata, KeyShardConfig, seal_share_for_guardian, split_backup_key,
};
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::DeliveryError;
use vauchi_core::sync::guardian_piece::{HandBackShare, split_backup_for_hand_back};
use vauchi_core::sync::guardian_transfer::HandBackSender;
use vauchi_core::{Command, Event, Vauchi};

fn fixture() -> (Vauchi, Vec<u8>, [u8; 32]) {
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let claim = phone.parse_recovery_claim_b64(&claim).unwrap();
    let recipient = vauchi_core::crypto::PublicKey::from_bytes(*claim.new_pk().as_bytes())
        .to_x25519()
        .unwrap();
    let shards = split_backup_key(
        &BackupKey::generate(),
        GuardianBackupMetadata::generate(KeyShardConfig::new(2, 3).unwrap()),
    )
    .unwrap();
    let backup = b"encrypted backup".to_vec();
    let hash = split_backup_for_hand_back(&backup).unwrap()[0].hash();
    let share = HandBackShare::new(
        1,
        seal_share_for_guardian(&shards[0], &recipient).unwrap(),
        hash,
    )
    .unwrap();
    phone.take_hand_back(&share.encode()).unwrap();
    (phone, backup, hash)
}

fn data(bytes: Vec<u8>, peer: &str) -> Event {
    Event::BleCharacteristicRead {
        device_id: peer.into(),
        direction: BleLinkDirection::Outbound,
        uuid: CHAR_DATA_NOTIFY.into(),
        data: bytes,
    }
}

fn disconnect() -> Command {
    Command::BleDisconnect {
        device_id: "peer".into(),
        direction: BleLinkDirection::Outbound,
    }
}

// @internal — P33: final ACK commands follow durable whole-backup validation.
#[test]
fn recovery_receiver_stores_before_ack_and_refuses_late_callbacks() {
    let (phone, backup, hash) = fixture();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    let callback = receiver.callback();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    assert_eq!(
        receiver.poll(&phone, 0),
        vec![Command::schedule_wakeup(300, 300, 0, None)]
    );
    while !sender.is_complete() {
        callback
            .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
            .unwrap();
        let commands = receiver.poll(&phone, 1);
        let Command::BleWriteCharacteristic {
            device_id,
            direction,
            uuid,
            data,
        } = &commands[0]
        else {
            panic!("missing ACK")
        };
        assert_eq!(device_id, "peer");
        assert_eq!(*direction, BleLinkDirection::Outbound);
        assert_eq!(uuid, CHAR_DATA_WRITE);
        sender.acknowledge(data).unwrap();
        if sender.is_complete() {
            assert_eq!(
                phone.storage().recovery().backup_pieces(&hash).unwrap(),
                vec![piece.encode()]
            );
            assert_eq!(commands.len(), 2);
            assert_eq!(commands[1], disconnect());
        } else {
            assert_eq!(commands.len(), 1);
        }
    }
    assert_eq!(receiver.state(), ReceiveState::Complete);
    assert_eq!(
        callback.publish(data(vec![], "peer")).unwrap_err().reason,
        DeliveryError::Inactive
    );
}

// @internal — DC-02: a database refusal cannot emit the final ACK.
#[cfg(feature = "testing")]
#[test]
fn recovery_receiver_storage_refusal_disconnects_without_final_ack() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 517).unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    receiver.poll(&phone, 0);
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only=ON")
        .unwrap();
    receiver
        .callback()
        .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
        .unwrap();
    assert_eq!(receiver.poll(&phone, 1), vec![disconnect()]);
    assert_eq!(receiver.state(), ReceiveState::Failed);
    assert_eq!(
        phone.storage().recovery().backup_pieces(&hash).unwrap(),
        Vec::<Vec<u8>>::new()
    );
}

// @internal — DC-02: cancellation invalidates queued bytes and the attempt scope.
#[test]
fn recovery_receiver_cancellation_and_timeout_reject_queued_bytes() {
    for cancel in [true, false] {
        let (phone, backup, hash) = fixture();
        let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
        let mut sender = HandBackSender::new(&piece, 517).unwrap();
        let mut receiver =
            RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
        let callback = receiver.callback();
        receiver.poll(&phone, 0);
        callback
            .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
            .unwrap();
        let commands = if cancel {
            receiver.cancel()
        } else {
            receiver.poll(&phone, 300)
        };
        assert_eq!(commands, vec![disconnect()]);
        assert_eq!(
            receiver.state(),
            if cancel {
                ReceiveState::Cancelled
            } else {
                ReceiveState::Failed
            }
        );
        assert_eq!(receiver.poll(&phone, 301), Vec::<Command>::new());
        assert_eq!(
            callback.publish(data(vec![], "peer")).unwrap_err().reason,
            DeliveryError::Inactive
        );
        assert_eq!(
            phone.storage().recovery().backup_pieces(&hash).unwrap(),
            Vec::<Vec<u8>>::new()
        );
    }
}

// @internal — DC-01/DC-02: only the selected peer, role and characteristic feed storage.
#[test]
fn recovery_receiver_rejects_unaccepted_or_malformed_frames_and_ignores_other_links() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 517).unwrap();
    let frame = sender.next_packet().unwrap().unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    receiver.poll(&phone, 0);
    for event in [
        data(frame.clone(), "other"),
        Event::BleCharacteristicRead {
            device_id: "peer".into(),
            direction: BleLinkDirection::Inbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: frame.clone(),
        },
        Event::BleCharacteristicRead {
            device_id: "peer".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_WRITE.into(),
            data: frame.clone(),
        },
    ] {
        receiver.callback().publish(event).unwrap();
        assert_eq!(receiver.poll(&phone, 1), Vec::<Command>::new());
        assert_eq!(receiver.state(), ReceiveState::Receiving);
    }
    assert_eq!(
        phone.storage().recovery().backup_pieces(&hash).unwrap(),
        Vec::<Vec<u8>>::new()
    );
    receiver.callback().publish(data(vec![], "peer")).unwrap();
    assert_eq!(receiver.poll(&phone, 2), vec![disconnect()]);
    assert_eq!(receiver.state(), ReceiveState::Failed);

    let without_share = Vauchi::in_memory().unwrap();
    without_share.begin_recovery(&"01".repeat(32)).unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    receiver.poll(&without_share, 0);
    receiver.callback().publish(data(frame, "peer")).unwrap();
    assert_eq!(receiver.poll(&without_share, 1), vec![disconnect()]);
    assert_eq!(receiver.state(), ReceiveState::Failed);
    assert_eq!(
        without_share
            .storage()
            .recovery()
            .backup_pieces(&hash)
            .unwrap(),
        Vec::<Vec<u8>>::new()
    );
}

// @internal — A11: an already stored backup does not acknowledge unsent bytes.
#[test]
fn recovery_receiver_replayed_piece_waits_for_its_final_frame() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    phone.take_hand_back_piece(&piece.encode()).unwrap();
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    receiver.poll(&phone, 0);
    receiver
        .callback()
        .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
        .unwrap();
    let commands = receiver.poll(&phone, 1);
    assert_eq!(commands.len(), 1);
    assert_eq!(receiver.state(), ReceiveState::Receiving);
}

// @internal — DC-01: a shell reports an inbound link with no id; a dialled link must name its peer.
#[test]
fn only_an_inbound_link_may_have_no_id() {
    let accepts = |peer: &str, direction| {
        RecoveryTransferReceiver::new([7; 32], peer.into(), direction).is_ok()
    };

    assert!(accepts("", BleLinkDirection::Inbound));
    assert!(!accepts("", BleLinkDirection::Outbound));
    assert!(accepts("peer", BleLinkDirection::Outbound));
    assert!(!accepts(&"x".repeat(129), BleLinkDirection::Inbound));
}

fn write_payload(commands: &[Command]) -> Vec<u8> {
    commands
        .iter()
        .find_map(|command| match command {
            Command::BleWriteCharacteristic { data, .. } => Some(data.clone()),
            _ => None,
        })
        .expect("an acknowledgement")
}

// @internal — P33: at the smallest MTU a piece takes minutes; only silence ends a slow transfer.
#[test]
fn a_transfer_that_keeps_progressing_outlives_the_attempt_limit() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    let callback = receiver.callback();
    receiver.poll(&phone, 0);

    let mut now = 0;
    while !sender.is_complete() {
        now += 200;
        callback
            .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
            .unwrap();
        let acknowledgement = write_payload(&receiver.poll(&phone, now));
        sender.acknowledge(&acknowledgement).unwrap();
    }

    assert!(now > 300, "the fixture must outlast the limit, took {now}");
    assert_eq!(receiver.state(), ReceiveState::Complete);
}

// @internal — P33: the timer armed at the start fires early once progress moved the limit.
#[test]
fn a_wakeup_before_the_moved_limit_rearms_for_the_time_left() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    let callback = receiver.callback();
    receiver.poll(&phone, 0);
    callback
        .publish(data(sender.next_packet().unwrap().unwrap(), "peer"))
        .unwrap();
    receiver.poll(&phone, 200);

    assert_eq!(
        receiver.poll(&phone, 300),
        vec![Command::schedule_wakeup(200, 200, 0, None)]
    );
    assert_eq!(receiver.state(), ReceiveState::Receiving);

    assert_eq!(receiver.poll(&phone, 500), vec![disconnect()]);
    assert_eq!(receiver.state(), ReceiveState::Failed);
}

// @internal — DC-02: a replayed frame is answered but is no progress, or a peer could hold the link open forever.
#[test]
fn a_replayed_frame_does_not_move_the_limit() {
    let (phone, backup, hash) = fixture();
    let piece = split_backup_for_hand_back(&backup).unwrap().remove(0);
    let mut sender = HandBackSender::new(&piece, 23).unwrap();
    let frame = sender.next_packet().unwrap().unwrap();
    let mut receiver =
        RecoveryTransferReceiver::new(hash, "peer".into(), BleLinkDirection::Outbound).unwrap();
    let callback = receiver.callback();
    receiver.poll(&phone, 0);
    callback.publish(data(frame.clone(), "peer")).unwrap();
    receiver.poll(&phone, 100);

    callback.publish(data(frame, "peer")).unwrap();
    assert_eq!(
        write_payload(&receiver.poll(&phone, 350)).len(),
        8,
        "the replay is still acknowledged"
    );

    assert_eq!(receiver.poll(&phone, 400), vec![disconnect()]);
    assert_eq!(receiver.state(), ReceiveState::Failed);
}

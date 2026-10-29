// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(target_os = "linux")]

use std::num::NonZeroUsize;
use std::sync::Arc;
use vauchi_core::platform::executor::InboxLimits;
use vauchi_core::platform::{BleAdvertisementObservation, BleServiceUuid, Event};
use vauchi_hardware::fake_bluez::{Call, FakeBlueZ, Script};
use vauchi_hardware::{BleScanner, BlueZDiscovery};

const SERVICE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const SERVICE_BYTES: [u8; 16] = [
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90,
];
const BATTERY: &str = "0000180f-0000-1000-8000-00805f9b34fb";
const BATTERY_BYTES: [u8; 16] = [
    0x00, 0x00, 0x18, 0x0f, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5f, 0x9b, 0x34, 0xfb,
];

fn scanner(bluez: &FakeBlueZ) -> BleScanner {
    BleScanner::with_backend(
        Arc::new(BlueZDiscovery::at(bluez.socket())),
        tokio::runtime::Handle::current(),
        InboxLimits {
            operations: NonZeroUsize::new(1).unwrap(),
            events: NonZeroUsize::new(4).unwrap(),
            event_bytes: NonZeroUsize::new(1024).unwrap(),
        },
    )
    .unwrap()
}

fn observed(address: &str, rssi: i16, uuids: &[[u8; 16]]) -> Event {
    Event::BleAdvertisementObserved(
        BleAdvertisementObservation::new(
            address.into(),
            rssi,
            uuids
                .iter()
                .copied()
                .map(BleServiceUuid::Normalized)
                .collect(),
        )
        .unwrap(),
    )
}

fn hardware_error() -> Event {
    Event::HardwareError {
        transport: "BLE".into(),
        error: "Bluetooth discovery failed".into(),
    }
}

async fn scanning(script: Script) -> (FakeBlueZ, BleScanner) {
    let mut bluez = FakeBlueZ::start(script).await;
    let mut scanner = scanner(&bluez);
    scanner.start(SERVICE.into()).unwrap();
    assert_eq!(
        bluez.calls.recv().await,
        Some(Call::SetDiscoveryFilter {
            uuids: vec![SERVICE.into()],
            transport: "le".into(),
        })
    );
    assert_eq!(bluez.calls.recv().await, Some(Call::StartDiscovery));
    (bluez, scanner)
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_device_advertising_the_service_is_observed_with_its_address_rssi_and_services() {
    let (bluez, mut scanner) = scanning(Script::default()).await;
    bluez
        .device_found("AA:BB:CC:DD:EE:01", -60, &[BATTERY, SERVICE])
        .await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed(
            "AA:BB:CC:DD:EE:01",
            -60,
            &[BATTERY_BYTES, SERVICE_BYTES]
        ))
    );
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn devices_without_the_service_are_not_observed() {
    let (bluez, mut scanner) = scanning(Script::default()).await;
    bluez
        .device_found("AA:BB:CC:DD:EE:02", -50, &[BATTERY])
        .await;
    bluez
        .device_found("AA:BB:CC:DD:EE:03", -70, &[SERVICE])
        .await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:03", -70, &[SERVICE_BYTES]))
    );
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_signal_strength_change_is_observed_again() {
    let (bluez, mut scanner) = scanning(Script::default()).await;
    bluez
        .device_found("AA:BB:CC:DD:EE:04", -80, &[SERVICE])
        .await;
    bluez.rssi_changed("AA:BB:CC:DD:EE:04", -40).await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:04", -80, &[SERVICE_BYTES]))
    );
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:04", -40, &[SERVICE_BYTES]))
    );
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_remembered_device_is_observed_only_once_its_signal_strength_arrives() {
    let (bluez, mut scanner) = scanning(Script::default()).await;
    bluez
        .device_remembered("AA:BB:CC:DD:EE:08", &[SERVICE])
        .await;
    bluez.rssi_changed("AA:BB:CC:DD:EE:08", -65).await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:08", -65, &[SERVICE_BYTES]))
    );
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn an_advertisement_beyond_the_observation_limits_is_skipped_and_scanning_continues() {
    let (bluez, mut scanner) = scanning(Script::default()).await;
    let crowded: Vec<String> = (0..40)
        .map(|n| format!("0000{n:04x}-0000-1000-8000-00805f9b34fb"))
        .chain([SERVICE.to_owned()])
        .collect();
    let crowded: Vec<&str> = crowded.iter().map(String::as_str).collect();
    bluez.device_found("AA:BB:CC:DD:EE:05", -30, &crowded).await;
    bluez
        .device_found("AA:BB:CC:DD:EE:06", -31, &[SERVICE])
        .await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:06", -31, &[SERVICE_BYTES]))
    );
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn stopping_closes_the_scan_connection_without_waiting_on_bluez() {
    let (mut bluez, mut scanner) = scanning(Script::default()).await;
    let connection = bluez.scanner().await;
    scanner.stop();
    bluez.disconnected(&connection).await;
    assert_eq!(bluez.calls.try_recv().ok(), None);
    scanner.shutdown().await.unwrap();
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_lost_bus_connection_is_reported_as_a_failure_not_an_empty_scan() {
    let (mut bluez, mut scanner) = scanning(Script::default()).await;
    bluez
        .device_found("AA:BB:CC:DD:EE:07", -55, &[SERVICE])
        .await;
    assert_eq!(
        scanner.next_event().await,
        Some(observed("AA:BB:CC:DD:EE:07", -55, &[SERVICE_BYTES]))
    );
    bluez.stop_bus();
    assert_eq!(scanner.next_event().await, Some(hardware_error()));
    scanner.shutdown().await.unwrap();
}

async fn failed_start(script: Script) -> Option<Event> {
    let bluez = FakeBlueZ::start(script).await;
    let mut scanner = scanner(&bluez);
    scanner.start(SERVICE.into()).unwrap();
    let event = scanner.next_event().await;
    scanner.shutdown().await.unwrap();
    event
}

fn unavailable() -> Option<Event> {
    Some(Event::HardwareUnavailable {
        transport: "BLE".into(),
    })
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn missing_bluez_adapter_or_power_reports_unavailable() {
    let without_bluez = Script {
        owns_name: false,
        ..Script::default()
    };
    let without_adapter = Script {
        adapter_powered: None,
        ..Script::default()
    };
    let powered_off = Script {
        adapter_powered: Some(false),
        ..Script::default()
    };
    assert_eq!(failed_start(without_bluez).await, unavailable());
    assert_eq!(failed_start(without_adapter).await, unavailable());
    assert_eq!(failed_start(powered_off).await, unavailable());
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn bluez_refusals_map_to_permission_unavailability_or_failure() {
    let refusing = |error| Script {
        start_error: Some(error),
        ..Script::default()
    };
    assert_eq!(
        failed_start(refusing("org.bluez.Error.NotAuthorized")).await,
        Some(Event::PermissionDenied {
            transport: "BLE".into()
        })
    );
    assert_eq!(
        failed_start(refusing("org.freedesktop.DBus.Error.AccessDenied")).await,
        Some(Event::PermissionDenied {
            transport: "BLE".into()
        })
    );
    assert_eq!(
        failed_start(refusing("org.bluez.Error.NotReady")).await,
        unavailable()
    );
    assert_eq!(
        failed_start(refusing("org.bluez.Error.Failed")).await,
        Some(hardware_error())
    );
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_service_id_that_is_not_a_uuid_fails_before_touching_bluez() {
    let mut bluez = FakeBlueZ::start(Script::default()).await;
    let mut scanner = scanner(&bluez);
    scanner.start("not-a-uuid".into()).unwrap();
    assert_eq!(scanner.next_event().await, Some(hardware_error()));
    scanner.shutdown().await.unwrap();
    assert_eq!(bluez.calls.try_recv().ok(), None);
}

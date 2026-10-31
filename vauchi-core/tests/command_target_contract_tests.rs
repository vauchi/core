// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every command names the shell that executes it (ADR-077): the OS's
//! hardware shell or the app's display shell. Shells route by the wire names
//! in `fixtures/command_targets_v1.json`, so Core's answer and the fixture
//! must agree.

use std::collections::BTreeSet;

use vauchi_core::{BleLinkDirection, Command, CommandTarget, DirectEndpoint, FilePickPurpose};

fn fixture_hardware_names() -> BTreeSet<String> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/command_targets_v1.json")).unwrap();
    fixture["hardware"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect()
}

fn wire_name(command: &Command) -> String {
    match serde_json::to_value(command).unwrap() {
        serde_json::Value::String(name) => name,
        serde_json::Value::Object(map) => map.keys().next().unwrap().clone(),
        other => panic!("unexpected command encoding {other}"),
    }
}

fn from_wire(json: serde_json::Value) -> Command {
    serde_json::from_value(json).unwrap()
}

fn hardware_commands() -> Vec<Command> {
    vec![
        Command::QrRequestScan,
        Command::ScheduleWakeup {
            earliest_secs: 60,
            deadline_secs: 120,
            min_interval_secs: 30,
            earliest_millis: None,
            delay_millis: 60_000,
        },
        Command::BleStartAdvertising {
            service_uuid: "uuid".into(),
            payload: vec![1],
        },
        Command::BleStartScanning {
            service_uuid: "uuid".into(),
        },
        Command::BleConnect {
            device_id: "device".into(),
        },
        Command::BleWriteCharacteristic {
            device_id: "device".into(),
            direction: BleLinkDirection::Outbound,
            uuid: "uuid".into(),
            data: vec![1],
        },
        Command::BleReadCharacteristic {
            device_id: "device".into(),
            direction: BleLinkDirection::Inbound,
            uuid: "uuid".into(),
        },
        Command::BleDisconnect {
            device_id: "device".into(),
            direction: BleLinkDirection::Outbound,
        },
        Command::BleStopScanning,
        Command::NfcActivate {
            payload: vec![],
            apdus: vec![vec![0x00]],
        },
        Command::NfcDeactivate,
        Command::NfcSendApdu {
            data: vec![0],
            apdus: vec![],
        },
        Command::AudioEmitChallenge {
            samples: vec![0.5],
            sample_rate: 44_100,
        },
        Command::AudioListenForResponse {
            timeout_ms: 1_000,
            sample_rate: 44_100,
        },
        Command::AudioStop,
        Command::AccelerometerStart,
        Command::AccelerometerStop,
        Command::RelayEscrowDeposit {
            gate_hash: vec![1],
            slot_hash: vec![2],
            encrypted_card: vec![3],
            ttl_seconds: 60,
        },
        Command::RelayEscrowCheck {
            gate_hash: vec![1],
            suggested_interval_ms: 500,
        },
        Command::RelayEscrowRetrieve {
            gate_hash: vec![1],
            slot_hash: vec![2],
        },
        Command::DirectSend {
            payload: vec![1],
            is_initiator: true,
            endpoint: DirectEndpoint::default(),
        },
        // Deprecated (ADR-066), so built from its wire form, as a shell
        // still receives it.
        from_wire(serde_json::json!({"DirectSendCard": {
            "ciphertext": [1], "is_initiator": false
        }})),
        Command::ImageCaptureFromCamera,
        Command::SwitchCamera { use_front: true },
        Command::LocationRequest { timeout_ms: 5_000 },
        from_wire(serde_json::json!({"PostNotification": {"notification": {
            "notification_id": "id", "title": "Title", "body": "Body",
            "deep_link_uri": null, "category_id": "category", "channel_id": "channel",
            "urgency": "default", "category_options": []
        }}})),
        Command::RequestBiometricUnlock,
        Command::ForgetStoredSecret {
            handle: "handle".into(),
        },
        from_wire(serde_json::json!({"CreateUserPresenceSecret": {
            "prompt": "Unlock", "binding": "biometry"
        }})),
        Command::RevealUserPresenceSecret {
            prompt: "Unlock".into(),
        },
        Command::SaveRestorableState {
            state: "state".into(),
        },
    ]
}

/// The display side of each boundary the owner drew on 2026-10-31: a QR to
/// show, the share sheet, pickers, screen settings for showing a QR, and
/// the celebration's haptic, sound and animation.
fn display_commands() -> Vec<Command> {
    vec![
        Command::QrDisplay { data: "qr".into() },
        Command::ShowShareSheet {
            url: "https://vauchi.app".into(),
        },
        Command::ImagePickFromLibrary,
        Command::ImagePickFromFile,
        Command::FilePickFromUser {
            accepted_mime_types: vec![],
            accepted_extensions: vec![],
            max_bytes: None,
            purpose: FilePickPurpose::ImportContacts,
        },
        Command::CancelFilePick {
            request_id: "request".into(),
        },
        Command::SetScreenBrightness { level: Some(1.0) },
        Command::SetIdleTimerDisabled { disabled: true },
        Command::SetOrientationLock { orientation: None },
        from_wire(serde_json::json!({"Celebrate": {
            "haptic": "success", "sound": "exchange_chime", "animation": "cards_meet"
        }})),
        Command::OpenExternalUrl {
            url: "https://vauchi.app".into(),
        },
        Command::PerformNativeBack,
        Command::ResetApplication,
        Command::ReloadTheme,
    ]
}

// @internal — the routing contract shells read (ADR-077).
#[test]
fn every_hardware_command_routes_to_the_hardware_shell() {
    for command in hardware_commands() {
        assert_eq!(
            command.target(),
            CommandTarget::Hardware,
            "{} must route to the hardware shell",
            wire_name(&command)
        );
    }
}

// @internal — the routing contract shells read (ADR-077).
#[test]
fn the_fixture_names_exactly_the_hardware_commands() {
    let routed: BTreeSet<String> = hardware_commands().iter().map(wire_name).collect();
    assert_eq!(routed, fixture_hardware_names());
}

// @internal — the routing contract shells read (ADR-077).
#[test]
fn display_commands_route_to_the_display_shell_and_are_absent_from_the_fixture() {
    let hardware = fixture_hardware_names();
    for command in display_commands() {
        let name = wire_name(&command);
        assert_eq!(
            command.target(),
            CommandTarget::Display,
            "{name} must route to the display shell"
        );
        assert!(
            !hardware.contains(&name),
            "{name} is a display command but the fixture lists it as hardware"
        );
    }
}

// @internal — shells decode the target name from the same JSON as commands.
#[test]
fn targets_encode_as_snake_case_names() {
    assert_eq!(
        serde_json::to_value(CommandTarget::Hardware).unwrap(),
        serde_json::json!("hardware")
    );
    assert_eq!(
        serde_json::to_value(CommandTarget::Display).unwrap(),
        serde_json::json!("display")
    );
}

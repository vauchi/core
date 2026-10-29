// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Removing my identity from this device: the device revokes itself, sends
//! that to my other devices in one sync outside the timing gate, then wipes
//! (vauchi/private#915, maintainer decisions 2026-10-29). Offline it stops
//! before wiping, so the user can try again or erase anyway; in duress
//! mode it is the emergency wipe and sends nothing.

use vauchi_core::api::{DeviceRemoval, Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::DeviceInfo;
use vauchi_core::network::OhttpClient;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

struct Phone {
    vauchi: Vauchi,
    _dir: tempfile::TempDir,
    outer: MockRelay,
}

/// My phone with a tablet linked to the same identity, reaching the relay
/// through `outer` when `online`.
fn phone_with_a_tablet(online: bool) -> Phone {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let dir = tempfile::tempdir().expect("temp dir");
    let outer_url = if online {
        outer.url()
    } else {
        "http://127.0.0.1:9".into()
    };
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer_url);
    let mut vauchi = Vauchi::new(config).expect("create Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    let identity = vauchi.identity().unwrap();
    let seed = *identity.master_seed();
    let mut registry = identity.initial_device_registry();
    registry
        .add_device(
            DeviceInfo::derive(&seed, 1, "tablet".into(), 0).to_registered(&seed),
            identity.signing_keypair(),
        )
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    Phone {
        vauchi,
        _dir: dir,
        outer,
    }
}

// @internal
#[test]
fn removing_my_identity_from_this_phone_sends_once_then_wipes() {
    let mut phone = phone_with_a_tablet(true);
    let before = phone.outer.received().len();

    let removal = phone
        .vauchi
        .remove_identity_from_this_device(false)
        .unwrap();

    assert_eq!(removal, DeviceRemoval::OtherDevicesTold);
    assert!(
        phone.outer.received().len() > before,
        "the removal must sync once"
    );
    assert!(phone.vauchi.identity().is_none(), "the phone is wiped");
}

// @internal
#[test]
fn offline_the_removal_stops_before_wiping_until_told_to_erase_anyway() {
    let mut phone = phone_with_a_tablet(false);

    let first = phone
        .vauchi
        .remove_identity_from_this_device(false)
        .unwrap();
    let kept = phone.vauchi.identity().is_some();
    let anyway = phone.vauchi.remove_identity_from_this_device(true).unwrap();

    assert_eq!(
        (first, kept, anyway, phone.vauchi.identity().is_none()),
        (
            DeviceRemoval::OtherDevicesNotTold,
            true,
            DeviceRemoval::OtherDevicesNotTold,
            true
        )
    );
}

// @internal
#[test]
fn removing_my_identity_in_duress_mode_is_the_emergency_wipe_and_sends_nothing() {
    let mut phone = phone_with_a_tablet(true);
    phone.vauchi.setup_app_password("app-pin-1234").unwrap();
    phone.vauchi.setup_duress_password("135790").unwrap();
    phone.vauchi.authenticate("135790").unwrap();
    let before = phone.outer.received().len();

    let removal = phone
        .vauchi
        .remove_identity_from_this_device(false)
        .unwrap();

    assert_eq!(removal, DeviceRemoval::Wiped);
    assert_eq!(phone.outer.received().len(), before, "duress sends nothing");
    assert!(phone.vauchi.identity().is_none(), "the phone is wiped");
}

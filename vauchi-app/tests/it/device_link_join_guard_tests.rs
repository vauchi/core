// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Joining another identity is only for a fresh install: a device that
//! already holds an identity refuses the invitation and keeps its data.

use vauchi_app::ui::AppEngine;
use vauchi_core::Identity;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{DeviceLinkJoinInvitation, DeviceLinkQR};

fn alices_invitation() -> String {
    let alice = Identity::create("Alice", 0);
    DeviceLinkJoinInvitation {
        qr_data: DeviceLinkQR::generate_with_timestamp(&alice, 1_700_000_000).to_data_string(),
        broker_code: "ABCD1234".into(),
        relay_url: None,
        local_rendezvous: None,
    }
    .to_url()
}

// @scenario: device_management :: Linking to a device that already has a different identity
#[test]
fn a_device_holding_bob_refuses_alices_invitation_and_keeps_bob() {
    let invitation = alices_invitation();
    let mut fresh = AppEngine::new(Vauchi::in_memory().unwrap());
    assert!(fresh.open_device_link_invitation(&invitation).is_ok());

    let mut bobs_device = Vauchi::in_memory().unwrap();
    bobs_device.create_identity("Bob").unwrap();
    let mut engine = AppEngine::new(bobs_device);

    let refused = engine.open_device_link_invitation(&invitation);

    assert_eq!(
        refused.map(|screen| screen.screen_id),
        Err("This device already has an identity.".to_string())
    );
    assert_eq!(
        engine.vauchi().own_card().unwrap().unwrap().display_name(),
        "Bob"
    );
}

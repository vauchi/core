// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads keyed by a contact id, and the failed-delivery list, under the
//! duress PIN (private#388). `get_contact` already treats a real contact's
//! id as unknown in duress mode (#386); these reads must answer the same
//! way, so nothing about the real contact book leaks through them
//! (ADR-032: an almost unused app).

use crate::common;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};
use vauchi_core::{AuthMode, Vauchi};

use common::helpers::setup_alice_bob_exchange;

fn avatar_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Real annotations and a failed delivery on Bob, then the duress unlock.
fn duress_with_real_annotations() -> (Vauchi, String /* bob_real_id */) {
    let (mut wb, _bob_wb, _secret, bob_id, _alice_id) = setup_alice_bob_exchange();
    wb.add_contact_shared_name(&bob_id, "Robert", true).unwrap();
    wb.add_contact_shared_avatar(&bob_id, &avatar_png(), true)
        .unwrap();
    wb.set_contact_nickname(&bob_id, "Bobby").unwrap();
    wb.set_contact_custom_avatar(&bob_id, &avatar_png())
        .unwrap();
    wb.add_personal_note(&bob_id, "met at the border").unwrap();
    wb.storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-real-1".into(),
            recipient_id: bob_id.clone(),
            status: DeliveryStatus::Failed {
                reason: "relay down".into(),
            },
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            expires_at: None,
        })
        .unwrap();
    // Precondition: every read sees the real data before the duress unlock.
    assert_eq!(wb.list_contact_shared_names(&bob_id).unwrap().len(), 1);
    assert_eq!(wb.get_failed_deliveries().unwrap().len(), 1);

    wb.setup_app_password("normal-pin").unwrap();
    wb.setup_duress_password("112233").unwrap();
    wb.add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .unwrap();
    assert_eq!(wb.authenticate("112233").unwrap(), AuthMode::Duress);
    (wb, bob_id)
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_duress_hides_real_shared_names_and_avatars() {
    let (wb, bob_id) = duress_with_real_annotations();

    assert_eq!(wb.list_contact_shared_names(&bob_id).unwrap().len(), 0);
    assert_eq!(wb.list_contact_shared_avatars(&bob_id).unwrap().len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_duress_hides_real_nickname_custom_avatar_and_note() {
    let (wb, bob_id) = duress_with_real_annotations();

    assert_eq!(wb.get_contact_nickname(&bob_id).unwrap(), None);
    assert_eq!(wb.get_contact_custom_avatar(&bob_id).unwrap(), None);
    assert_eq!(wb.read_personal_note(&bob_id).unwrap(), None);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_duress_real_contact_display_options_are_not_found() {
    let (wb, bob_id) = duress_with_real_annotations();

    let err = wb
        .get_contact_display_options(&bob_id)
        .expect_err("a real contact's display options must not resolve");
    assert!(
        matches!(err, vauchi_core::VauchiError::ContactNotFound(ref id) if id == &bob_id),
        "got {err:?}"
    );
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_duress_hides_real_deliveries() {
    let (wb, bob_id) = duress_with_real_annotations();

    assert_eq!(
        wb.get_delivery_status_for_contact(&bob_id).unwrap().len(),
        0
    );
    assert_eq!(wb.get_failed_deliveries().unwrap().len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_normal_unlock_still_reads_the_real_annotations() {
    let (mut wb, bob_id) = duress_with_real_annotations();
    assert_eq!(wb.authenticate("normal-pin").unwrap(), AuthMode::Normal);

    assert_eq!(
        wb.list_contact_shared_names(&bob_id).unwrap()[0].name,
        "Robert"
    );
    assert_eq!(
        wb.get_contact_nickname(&bob_id).unwrap().as_deref(),
        Some("Bobby")
    );
    assert_eq!(
        wb.read_personal_note(&bob_id).unwrap().as_deref(),
        Some("met at the border")
    );
    assert_eq!(
        wb.get_failed_deliveries().unwrap()[0].message_id,
        "msg-real-1"
    );
}

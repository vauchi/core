// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Factory-level wiring test: the Cable (DirectTransport) engine built by
//! [`AppEngine::create_engine`] transmits only the selected group's visible
//! fields. Lives in its own file (not inline in `screens.rs`) to keep that
//! grandfathered factory from growing. Accesses the `pub(super)`
//! `create_engine` factory and the test-only
//! `DirectTransportEngine::outgoing_card` seam — both crate-internal, so this
//! stays in the `src` tree.
//!
//! Problem: 2026-06-08-exchange-card-not-group-filtered (G2 Slice 3).

// INLINE_TEST_REQUIRED: exercises the `pub(super)` create_engine factory and
// the `pub(crate)` DirectTransportEngine::outgoing_card seam — both
// crate-internal, so this cannot live in a `tests/` integration directory.
use super::AppEngine;
use crate::ui::AppScreen;
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};

// @internal
#[test]
fn cable_engine_transmits_only_selected_group_fields() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut card = vauchi
        .own_card()
        .expect("own_card")
        .expect("create_identity saves a card");
    card.add_field(ContactField::new(FieldType::Email, "Email", "a@b.com", 0))
        .expect("add email");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+12025550123",
        0,
    ))
    .expect("add phone");
    vauchi.update_own_card(&card).expect("update own card");
    let email_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Email")
        .expect("email field")
        .id()
        .to_string();
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    vauchi
        .set_group_field_visibility(&work_id, &email_id, true)
        .expect("expose email to Work");

    let caps = vauchi_core::exchange::capability::types::DeviceCapabilities::default();
    let readiness = vauchi_core::exchange::capability::TransportReadiness::default();
    let ctx = crate::ui::RenderContext::default();
    let engine = AppEngine::create_engine(
        &vauchi,
        &AppScreen::DirectTransport,
        None,
        &caps,
        &readiness,
        &ctx,
        &[work_id],
    );
    let dt = engine
        .as_any()
        .and_then(|a| a.downcast_ref::<crate::ui::DirectTransportEngine>())
        .expect("DirectTransportEngine");
    let labels: Vec<&str> = dt
        .outgoing_card()
        .expect("live session has an outgoing card")
        .fields()
        .iter()
        .map(|f| f.label())
        .collect();
    assert_eq!(
        labels,
        vec!["Email"],
        "Cable must transmit only Work-visible fields; got {labels:?}"
    );
}

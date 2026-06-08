// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C-01: Duress mode contact API bypass tests
//!
//! Verifies that ALL contact query functions respect `auth_mode`,
//! returning only decoy contacts when in duress mode. Previously
//! only `list_contacts()` was guarded; the other functions bypassed
//! duress and exposed the real contact database.
//!
//! ADR-032: "When a user unlocks with the duress PIN, decoy contacts
//! are loaded and shown as real contacts."

use crate::common;

use vauchi_core::AuthMode;
use vauchi_core::contact_card::ContactCard;

use common::helpers::setup_alice_bob_exchange;

/// Sets up duress mode: password, duress PIN, one decoy contact.
/// Returns the Vauchi instance (in duress mode) and Bob's real contact ID.
fn setup_duress_with_decoy() -> (vauchi_core::Vauchi, String /* bob_real_id */) {
    let (mut alice_wb, _bob_wb, _secret, bob_id, _alice_id) = setup_alice_bob_exchange();

    alice_wb
        .setup_app_password("normal-pin")
        .expect("setup app password");
    alice_wb
        .setup_duress_password("duress-pin")
        .expect("setup duress");

    let decoy_card = ContactCard::new("Decoy Dana");
    alice_wb
        .add_decoy_contact("decoy-dana", "Decoy Dana", &decoy_card)
        .expect("add decoy");

    let mode = alice_wb.authenticate("duress-pin").expect("auth");
    assert_eq!(mode, AuthMode::Duress);

    (alice_wb, bob_id)
}

// =============================================================================
// C-01: get_contact must respect duress mode
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_get_contact_duress_hides_real_contacts() {
    let (wb, bob_id) = setup_duress_with_decoy();

    // Real contact (Bob) must NOT be accessible in duress mode
    let result = wb
        .get_contact(&bob_id)
        .expect("get_contact should not error");
    assert!(
        result.is_none(),
        "get_contact({}) must return None in duress mode — real contacts must be hidden",
        bob_id
    );
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn test_get_contact_duress_returns_decoy() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    // Decoy contact IDs are derived (SHA256 of storage ID, hex-encoded).
    // Look up via list_contacts first to get the actual ID.
    let contacts = wb.list_contacts().expect("list should work");
    assert_eq!(contacts.len(), 1);
    let decoy_id = contacts[0].id().to_string();

    let result = wb
        .get_contact(&decoy_id)
        .expect("get_contact should not error");
    assert!(
        result.is_some(),
        "get_contact with decoy ID must return the decoy in duress mode"
    );
    assert_eq!(result.unwrap().display_name(), "Decoy Dana");
}

// =============================================================================
// C-01: list_contacts_paginated must respect duress mode
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_list_contacts_paginated_duress_returns_decoys_only() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    let contacts = wb
        .list_contacts_paginated(0, 100)
        .expect("list_contacts_paginated should succeed");

    // Must return only the decoy, not real Bob
    assert_eq!(
        contacts.len(),
        1,
        "paginated list must return only decoy contacts in duress mode"
    );
    assert_eq!(contacts[0].display_name(), "Decoy Dana");
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn test_list_contacts_paginated_duress_pagination_works() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    // Offset past the single decoy
    let contacts = wb
        .list_contacts_paginated(1, 100)
        .expect("paginated list should succeed");
    assert!(
        contacts.is_empty(),
        "paginated list with offset past decoys must return empty"
    );
}

// =============================================================================
// C-01: search_contacts must respect duress mode
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_search_contacts_duress_does_not_find_real() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    // Searching for the real contact's name must return empty
    let results = wb
        .search_contacts("Bob")
        .expect("search_contacts should succeed");
    assert!(
        results.is_empty(),
        "search_contacts('Bob') must return empty in duress mode — real contacts hidden"
    );
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn test_search_contacts_duress_finds_decoy() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    let results = wb
        .search_contacts("Dana")
        .expect("search_contacts should succeed");
    assert_eq!(
        results.len(),
        1,
        "search_contacts('Dana') must find the decoy contact in duress mode"
    );
    assert_eq!(results[0].display_name(), "Decoy Dana");
}

// =============================================================================
// C-01: find_contact_fuzzy must respect duress mode
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_find_contact_fuzzy_duress_does_not_find_real() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    let results = wb
        .find_contact_fuzzy("Bob")
        .expect("find_contact_fuzzy should succeed");
    assert!(
        results.is_empty(),
        "find_contact_fuzzy('Bob') must return empty in duress mode"
    );
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn test_find_contact_fuzzy_duress_finds_decoy() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    let results = wb
        .find_contact_fuzzy("Dana")
        .expect("find_contact_fuzzy should succeed");
    assert_eq!(
        results.len(),
        1,
        "find_contact_fuzzy('Dana') must find the decoy in duress mode"
    );
}

// =============================================================================
// C-01: contact_count must respect duress mode
// =============================================================================

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn test_contact_count_duress_returns_decoy_count() {
    let (wb, _bob_id) = setup_duress_with_decoy();

    let count = wb.contact_count().expect("contact_count should succeed");
    assert_eq!(
        count, 1,
        "contact_count must return decoy count (1), not real count, in duress mode"
    );
}

// =============================================================================
// Negative: normal mode still works
// =============================================================================

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn test_all_apis_normal_mode_still_return_real() {
    let (mut alice_wb, _bob_wb, _secret, bob_id, _alice_id) = setup_alice_bob_exchange();

    alice_wb.setup_app_password("normal-pin").expect("setup");
    alice_wb
        .setup_duress_password("duress-pin")
        .expect("setup duress");

    let decoy_card = ContactCard::new("Decoy Dana");
    alice_wb
        .add_decoy_contact("decoy-dana", "Decoy Dana", &decoy_card)
        .expect("add decoy");

    // Authenticate normally
    let mode = alice_wb.authenticate("normal-pin").expect("auth");
    assert_eq!(mode, AuthMode::Normal);

    // All APIs must return real contacts, not decoys
    let contact = alice_wb.get_contact(&bob_id).expect("get");
    assert!(
        contact.is_some(),
        "get_contact must find Bob in normal mode"
    );

    let paginated = alice_wb.list_contacts_paginated(0, 100).expect("paginated");
    assert!(!paginated.is_empty(), "paginated must return real contacts");

    let searched = alice_wb.search_contacts("Bob").expect("search");
    assert!(!searched.is_empty(), "search must find Bob in normal mode");

    let count = alice_wb.contact_count().expect("count");
    assert!(
        count >= 1,
        "count must include real contacts in normal mode"
    );
}

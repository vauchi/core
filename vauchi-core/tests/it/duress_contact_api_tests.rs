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
        .setup_duress_password("112233")
        .expect("setup duress");

    let decoy_card = ContactCard::new("Decoy Dana");
    alice_wb
        .add_decoy_contact("decoy-dana", "Decoy Dana", &decoy_card)
        .expect("add decoy");

    let mode = alice_wb.authenticate("112233").expect("auth");
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
        .setup_duress_password("112233")
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

// =============================================================================
// #386: contact reads beyond the C-01 set must not reveal real state under
// duress. Decoy mode must read as an almost unused app (ADR-032 addenda), so
// real archived / hidden / blocked contacts, label members, and duplicate
// pairs are all absent, and search covers decoys only.
// =============================================================================

/// Duress setup where `prepare` shapes real state before the duress unlock.
fn setup_duress_after(
    prepare: impl FnOnce(&vauchi_core::Vauchi, &str),
) -> (vauchi_core::Vauchi, String /* bob_real_id */) {
    let (mut alice_wb, _bob_wb, _secret, bob_id, _alice_id) = setup_alice_bob_exchange();
    prepare(&alice_wb, &bob_id);
    alice_wb
        .setup_app_password("normal-pin")
        .expect("setup app password");
    alice_wb
        .setup_duress_password("112233")
        .expect("setup duress");
    alice_wb
        .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .expect("add decoy");
    assert_eq!(
        alice_wb.authenticate("112233").expect("auth"),
        AuthMode::Duress
    );
    (alice_wb, bob_id)
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_list_archived_contacts_duress_hides_real_archive() {
    let (wb, _) = setup_duress_after(|wb, bob| wb.archive_contact(bob).expect("archive"));

    assert_eq!(wb.list_archived_contacts().expect("list").len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_list_hidden_contacts_duress_hides_real_hidden() {
    let (wb, _) = setup_duress_after(|wb, bob| wb.hide_contact(bob).expect("hide"));

    assert_eq!(wb.list_hidden_contacts().expect("list").len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_list_blocked_contacts_duress_hides_real_blocked() {
    let (wb, _) = setup_duress_after(|wb, bob| wb.block_contact(bob).expect("block"));

    assert_eq!(wb.list_blocked_contacts().expect("list").len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_group_members_duress_hides_real_members() {
    let label = std::cell::RefCell::new(String::new());
    let (wb, _) = setup_duress_after(|wb, bob| {
        let group = wb.create_group("Family").expect("group");
        wb.add_contact_to_group(group.id(), bob).expect("member");
        *label.borrow_mut() = group.id().to_string();
    });

    let members = wb.get_group_members(&label.borrow()).unwrap_or_default();
    assert_eq!(members.len(), 0, "real label members leaked: {members:?}");
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_find_duplicates_duress_hides_real_pairs() {
    let (wb, _) = setup_duress_after(|wb, bob| {
        let name = wb
            .get_contact(bob)
            .expect("get")
            .expect("bob")
            .display_name()
            .to_string();
        let twin = vauchi_core::Contact::from_exchange(
            [42u8; 32],
            ContactCard::new(&name),
            vauchi_core::SymmetricKey::generate(),
            0,
        );
        wb.add_contact(twin).expect("twin");
        assert!(
            !wb.find_duplicates().expect("dupes").is_empty(),
            "precondition: the real twins are detected as duplicates"
        );
    });

    assert_eq!(wb.find_duplicates().expect("dupes").len(), 0);
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_search_contacts_faceted_duress_covers_decoys_only() {
    let (wb, bob_id) = setup_duress_after(|_, _| {});
    let facets = vauchi_core::api::SearchFacets::default();

    let real = wb.search_contacts_faceted("", &facets).expect("search all");
    assert!(
        real.iter().all(|contact| contact.id() != bob_id),
        "faceted search leaked the real contact"
    );
    let decoys = wb
        .search_contacts_faceted("Decoy", &facets)
        .expect("search decoy");
    assert_eq!(
        decoys
            .iter()
            .map(|contact| contact.display_name())
            .collect::<Vec<_>>(),
        ["Decoy Dana"]
    );
}

// =============================================================================
// Personal-data export follows the auth mode (private#576)
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_export_personal_data_duress_holds_decoys_only() {
    let (wb, bob_id) = setup_duress_with_decoy();
    wb.storage()
        .consent()
        .log_audit_event("real_owner_event", None)
        .unwrap();

    let export = wb
        .export_personal_data()
        .expect("export_personal_data should succeed");

    let names: Vec<&str> = export
        .contacts
        .iter()
        .map(|c| c.display_name.as_str())
        .collect();
    assert_eq!(names, ["Decoy Dana"], "only the decoy may be exported");
    assert!(
        export.audit_log.is_empty(),
        "the decoy profile exports no audit log"
    );
    assert_eq!(
        export
            .recovery_config
            .as_ref()
            .map(|r| r.trusted_contacts_count),
        Some(0),
        "the trusted-contact count must not come from real contacts"
    );
    let json = serde_json::to_string(&export).unwrap();
    assert!(
        !json.contains(&bob_id),
        "export holds a real contact id: {json}"
    );
}

// @scenario: privacy_compliance :: Export all my data
#[test]
fn test_export_personal_data_normal_mode_holds_real_contacts() {
    let (alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();
    alice_wb
        .storage()
        .consent()
        .log_audit_event("real_owner_event", None)
        .unwrap();

    let export = alice_wb
        .export_personal_data()
        .expect("export_personal_data should succeed");

    assert_eq!(export.contacts.len(), 1, "the real contact is exported");
    let json = serde_json::to_string(&export.audit_log).unwrap();
    assert!(
        json.contains("real_owner_event"),
        "normal mode keeps the real audit log: {json}"
    );
}

// =============================================================================
// The encrypted personal-data export follows the auth mode too (private#616)
// =============================================================================

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn test_export_personal_data_encrypted_duress_holds_decoys_only() {
    let (wb, bob_id) = setup_duress_with_decoy();
    wb.storage()
        .consent()
        .log_audit_event("real_owner_event", None)
        .unwrap();

    let encrypted = wb
        .export_personal_data_encrypted("export-password")
        .expect("the encrypted export should succeed");
    let export = vauchi_core::api::import_encrypted(&encrypted, "export-password")
        .expect("the export decrypts with its password");

    let names: Vec<&str> = export
        .contacts
        .iter()
        .map(|c| c.display_name.as_str())
        .collect();
    assert_eq!(names, ["Decoy Dana"], "only the decoy may be exported");
    assert!(
        export.audit_log.is_empty(),
        "the decoy profile exports no audit log"
    );
    let json = serde_json::to_string(&export).unwrap();
    assert!(
        !json.contains(&bob_id),
        "encrypted export holds a real contact id: {json}"
    );
}

// @scenario: privacy_compliance :: Export all my data
#[test]
fn test_export_personal_data_encrypted_normal_mode_holds_real_contacts() {
    let (alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();
    let plain = alice_wb.export_personal_data().unwrap();

    let encrypted = alice_wb
        .export_personal_data_encrypted("export-password")
        .expect("the encrypted export should succeed");
    let export = vauchi_core::api::import_encrypted(&encrypted, "export-password")
        .expect("the export decrypts with its password");

    assert_eq!(export.contacts.len(), 1, "the real contact is exported");
    assert_eq!(
        export.contacts[0].display_name, plain.contacts[0].display_name,
        "the encrypted export holds the same real contact as the plain one"
    );
    assert_eq!(
        export.contacts[0].public_key_fingerprint,
        plain.contacts[0].public_key_fingerprint
    );
    assert!(
        vauchi_core::api::import_encrypted(&encrypted, "wrong-password").is_err(),
        "a wrong password does not decrypt the export"
    );
}

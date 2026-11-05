// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::Vauchi;

// @internal — P32: reading resume state cannot create a recovery or expose it after initialization.
#[test]
fn pending_recovery_claim_requires_identity_free_persisted_recovery() {
    let mut phone = Vauchi::in_memory().unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    let resumed = phone.pending_recovery_claim().unwrap().unwrap();
    let resumed = phone.parse_recovery_claim_b64(&resumed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!phone.has_identity());

    phone.create_identity("Alice").unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    assert_eq!(phone.identity().unwrap().display_name(), "Alice");
}

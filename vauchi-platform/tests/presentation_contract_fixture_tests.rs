// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn uniffi_returns_the_core_owned_fixture_bytes() {
    assert_eq!(
        vauchi_platform::presentation_contract_fixture_json(),
        vauchi_app::ui::presentation_contract_fixture_json()
    );
    let fixture: serde_json::Value =
        serde_json::from_str(&vauchi_platform::presentation_contract_fixture_json()).unwrap();
    assert_eq!(
        fixture["effect_commands"][0]["PostNotification"]["notification"]["notification_id"],
        "opaque-notification-17"
    );
    assert_eq!(
        fixture["file_picker_contract"]["commands"][0]["PickFile"]["request_id"],
        "opaque-file-1"
    );
}

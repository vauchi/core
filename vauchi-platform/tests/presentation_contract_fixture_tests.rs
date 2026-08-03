// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

#[test]
fn uniffi_returns_the_core_owned_fixture_bytes() {
    assert_eq!(
        vauchi_platform::presentation_contract_fixture_json(),
        vauchi_app::ui::presentation_contract_fixture_json()
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Contact Limit screen is removed (#467, owner decision 2026-10-01):
//! it never read or saved the limit, and core always has one (10,000 by
//! default), so there was nothing honest for it to show. The limit itself
//! stays internal and enforced.

use vauchi_app::ui::AppScreen;
use vauchi_core::Vauchi;

// @internal
#[test]
fn the_contact_limit_screen_is_gone() {
    assert_eq!(AppScreen::from_screen_id("contact_limit"), None);
}

// @internal
#[test]
fn the_limit_is_still_enforced_internally() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    assert_eq!(vauchi.get_contact_limit().unwrap(), 10_000);
}

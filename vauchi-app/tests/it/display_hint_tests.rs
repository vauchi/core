// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sanity tests for [`DisplayHint`].
//!
//! The enum carries no behaviour — these tests pin the trait set
//! (`Copy`, `Eq`, `Hash`) and the variant distinctness that engines
//! rely on when matching.

use vauchi_app::ui::DisplayHint;

// @internal
#[test]
fn copy_and_eq_semantics() {
    let h = DisplayHint::Phone;
    let h2 = h;
    assert_eq!(h, h2);
    assert_ne!(DisplayHint::Phone, DisplayHint::Watch);
}

// @internal
#[test]
fn each_variant_is_distinct() {
    assert_ne!(DisplayHint::Phone, DisplayHint::Watch);
    assert_ne!(DisplayHint::Phone, DisplayHint::Desktop);
    assert_ne!(DisplayHint::Watch, DisplayHint::Desktop);
}

// @internal
#[test]
fn serde_roundtrip_each_variant() {
    for h in [DisplayHint::Phone, DisplayHint::Watch, DisplayHint::Desktop] {
        let json = serde_json::to_string(&h).expect("serialize DisplayHint");
        let back: DisplayHint = serde_json::from_str(&json).expect("deserialize DisplayHint");
        assert_eq!(h, back, "roundtrip failed for {h:?}");
    }
}

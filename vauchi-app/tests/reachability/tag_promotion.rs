// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `TagPromotionEngine` (ADR-051, Phase 4b).
//!
//! The only diffed `ActionPressed` affordance is `confirm_promotion`; the
//! field-review toggles emit `ItemToggled`, which the walker does not diff.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{PromotionField, TagPromotionEngine, WorkflowEngine};

const HANDLED: &[&str] = &["confirm_promotion"];

fn factory() -> TagPromotionEngine {
    TagPromotionEngine::new(
        "t1".into(),
        "climbing".into(),
        2,
        vec![
            PromotionField {
                field_id: "f1".into(),
                label: "Email".into(),
                value: "a@b.c".into(),
                selected: true,
            },
            PromotionField {
                field_id: "f2".into(),
                label: "Phone".into(),
                value: "123".into(),
                selected: false,
            },
        ],
    )
}

// @internal
#[test]
fn tag_promotion_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "tag_promotion");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn tag_promotion_screen_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}

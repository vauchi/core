// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Adding an own-card field with group visibility must not report
//! success when a group grant did not land.
//!
//! The add-field form lets the user tick groups that should see the new
//! detail. Granting happens after the field is saved, so a group that
//! disappears between opening the form and pressing save leaves the user
//! believing they shared something nobody can see. The engine must say
//! so rather than navigating back as if everything worked.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn act(engine: &mut AppEngine, action: UserAction) {
    let _ = engine.handle_action(action);
}

/// Open the add-field form from MyInfo and fill in a phone number,
/// ticking every group in `groups` for visibility. Stops before save.
fn fill_add_field_form(engine: &mut AppEngine, value: &str, groups: &[String]) {
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "add_field".into(),
        },
    );
    act(
        engine,
        UserAction::ListItemSelected {
            component_id: "entry_types".into(),
            item_id: "phone".into(),
        },
    );
    act(
        engine,
        UserAction::TextChanged {
            component_id: "field_value".into(),
            value: value.into(),
        },
    );
    for group_id in groups {
        act(
            engine,
            UserAction::ItemToggled {
                component_id: "group_visibility".into(),
                item_id: group_id.clone(),
            },
        );
    }
}

fn own_card_field_values(engine: &AppEngine) -> Vec<String> {
    engine
        .vauchi()
        .own_card()
        .expect("own card loads")
        .expect("identity exists")
        .fields()
        .iter()
        .map(|field| field.value().to_string())
        .collect()
}

// @scenario: visibility_labels.feature :: A field that could not be shared with a label says so
#[test]
fn add_field_reports_groups_that_did_not_receive_the_new_detail() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Ana").expect("identity created");
    let group_id = vauchi
        .create_group("Work")
        .expect("group created")
        .id()
        .to_string();

    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::MyInfo);
    fill_add_field_form(
        &mut engine,
        "+41 79 000 00 00",
        std::slice::from_ref(&group_id),
    );

    // The group goes away while the form is still open — the race the
    // grant loop used to swallow.
    engine
        .vauchi()
        .delete_group(&group_id)
        .expect("group deleted");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    let ActionResult::ShowAlert { title, message } = result else {
        panic!("a grant that did not land must be reported, got {result:?}");
    };
    assert_eq!(title, "Saved, but not shared");
    assert_eq!(
        message,
        "Your new detail was saved, but 1 of the groups you picked did \
         not get it. Open the group to share it."
    );

    assert_eq!(
        own_card_field_values(&engine),
        vec!["+41 79 000 00 00".to_string()],
        "the detail itself was saved — only the grant failed"
    );
}

// @scenario: visibility_labels.feature :: Associate a field with a label
#[test]
fn add_field_navigates_back_when_every_group_grant_lands() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Ana").expect("identity created");
    let group_id = vauchi
        .create_group("Work")
        .expect("group created")
        .id()
        .to_string();

    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::MyInfo);
    fill_add_field_form(
        &mut engine,
        "+41 79 111 11 11",
        std::slice::from_ref(&group_id),
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "a fully granted field returns to the parent screen, got {result:?}"
    );
    assert_eq!(
        own_card_field_values(&engine),
        vec!["+41 79 111 11 11".to_string()],
    );
    assert!(
        engine
            .vauchi()
            .get_group(&group_id)
            .expect("group loads")
            .visible_fields()
            .len()
            == 1,
        "the group received exactly the one field that was granted"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where a screen's decision goes: into the bar's pinned group for a
//! shell that keeps it in view, or left in the content for one that does
//! not (vauchi/private#1304).

use super::AppEngine;
use crate::ui::{ActionListItem, Component, MAX_PINNED_ACTIONS, ScreenModel};

impl AppEngine {
    /// Takes the screen's pinned list out of its content when the shell
    /// said it keeps pinned actions in view. `ContextualSurface` then
    /// offers the items in `ContextBar::pinned`.
    pub(super) fn apply_pinned_actions(&self, mut screen: ScreenModel) -> ScreenModel {
        if !self.presentation_coordinator.pins_actions() {
            return screen;
        }
        let Some(mut pinned) = screen.pinned_list.take() else {
            return screen;
        };
        if let Some(items) = take_button_list(&mut screen.components, &pinned.component_id) {
            pinned.lifted = items;
        }
        screen.pinned_list = Some(pinned);
        screen
    }
}

/// Removes the button list `id` from `components`, looking inside rows and
/// columns, and returns its items. A list too long to pin stays where it is.
fn take_button_list(components: &mut Vec<Component>, id: &str) -> Option<Vec<ActionListItem>> {
    let found = components.iter().position(|component| {
        matches!(
            component,
            Component::ButtonList { id: list_id, items }
                if list_id == id && (1..=MAX_PINNED_ACTIONS).contains(&items.len())
        )
    });
    if let Some(index) = found {
        return match components.remove(index) {
            Component::ButtonList { items, .. } => Some(items),
            _ => None,
        };
    }
    components.iter_mut().find_map(|component| match component {
        Component::Row { items, .. } | Component::Column { items, .. } => {
            take_button_list(items, id)
        }
        _ => None,
    })
}

// INLINE_TEST_REQUIRED: no screen names a list of four yet, so the length
// limit has no way in through the engine.
#[cfg(test)]
mod tests {
    use super::*;

    fn list(id: &str, items: usize) -> Component {
        Component::ButtonList {
            id: id.into(),
            items: (0..items)
                .map(|index| ActionListItem {
                    id: format!("item_{index}"),
                    label: format!("Item {index}"),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                })
                .collect(),
        }
    }

    fn ids(items: &[ActionListItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    // @internal
    #[test]
    fn a_list_inside_a_row_is_taken_out_with_its_items_in_order() {
        let mut components = vec![Component::Row {
            id: "row".into(),
            items: vec![list("other", 1), list("answers", 3)],
        }];

        let taken = take_button_list(&mut components, "answers").expect("the list");

        assert_eq!(ids(&taken), ["item_0", "item_1", "item_2"]);
        assert_eq!(
            components,
            vec![Component::Row {
                id: "row".into(),
                items: vec![list("other", 1)],
            }]
        );
    }

    // @internal
    #[test]
    fn a_list_of_four_stays_in_the_content() {
        let mut components = vec![list("answers", MAX_PINNED_ACTIONS + 1)];

        let taken = take_button_list(&mut components, "answers");

        assert_eq!(taken, None);
        assert_eq!(components, vec![list("answers", MAX_PINNED_ACTIONS + 1)]);
    }

    // @internal
    #[test]
    fn an_empty_list_is_left_alone() {
        let mut components = vec![list("answers", 0)];

        assert_eq!(take_button_list(&mut components, "answers"), None);
        assert_eq!(components, vec![list("answers", 0)]);
    }
}

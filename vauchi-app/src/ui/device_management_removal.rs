// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Removing the identity from this device, on the Devices screen
//! (vauchi/private#915; maintainer decision 2026-10-29: here, behind a
//! typed confirmation, because the wipe it ends in keeps the shred screen's
//! guard). With no other active device it explains instead; when the
//! other devices could not be told it offers a retry or erasing anyway.

// INLINE_TEST_REQUIRED: Tests access the engine's private removal state
#[cfg(test)]
mod tests {
    use super::super::*;

    fn device(index: u32, current: bool, active: bool) -> DeviceListItem {
        DeviceListItem {
            device_index: index,
            device_name: format!("device {index}"),
            public_key_prefix: "a1b2c3d4".into(),
            is_current: current,
            is_active: active,
        }
    }

    fn with_a_tablet() -> DeviceManagementEngine {
        DeviceManagementEngine::new(vec![device(0, true, true), device(1, false, true)])
    }

    fn alone() -> DeviceManagementEngine {
        DeviceManagementEngine::new(vec![device(0, true, true), device(1, false, false)])
    }

    fn press(e: &mut DeviceManagementEngine, id: &str) -> ActionResult {
        e.handle_action(UserAction::ActionPressed {
            action_id: id.into(),
        })
    }

    fn type_word(e: &mut DeviceManagementEngine, word: &str) {
        let _ = e.handle_action(UserAction::TextChanged {
            component_id: "confirmation".into(),
            value: word.into(),
        });
    }

    fn removal_requested(e: &DeviceManagementEngine) -> Option<bool> {
        match e.engine_output() {
            Some(crate::ui::EngineOutput::DeviceManagement {
                remove_this_device, ..
            }) => remove_this_device,
            _ => None,
        }
    }

    fn action_ids(screen: &ScreenModel) -> Vec<String> {
        screen
            .contextual_actions
            .iter()
            .map(|a| a.id.clone())
            .collect()
    }

    fn confirm_ids(screen: &ScreenModel) -> Vec<String> {
        screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::InlineConfirm {
                    confirm_action_id,
                    cancel_action_id,
                    ..
                } => Some([confirm_action_id.clone(), cancel_action_id.clone()]),
                _ => None,
            })
            .flatten()
            .collect()
    }

    // @internal
    #[test]
    fn the_devices_screen_offers_removing_the_identity_from_this_device_as_serious() {
        let screen = with_a_tablet().current_screen();
        let remove = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "remove_this_device")
            .expect("remove_this_device action present");
        assert_eq!(remove.style, ActionStyle::Serious);
    }

    // @internal
    #[test]
    fn removing_asks_for_the_typed_word_before_it_hands_over() {
        let mut e = with_a_tablet();
        let _ = press(&mut e, "remove_this_device");
        let screen = e.current_screen();
        let ids = confirm_ids(&screen);

        let wrong = press(&mut e, "confirm_remove_this_device");
        let after_wrong = removal_requested(&e);
        type_word(&mut e, "wipe ");
        let right = press(&mut e, "confirm_remove_this_device");

        assert_eq!(
            (
                screen.screen_id.as_str(),
                ids,
                matches!(wrong, ActionResult::ValidationError { .. }),
                after_wrong,
                matches!(right, ActionResult::Complete),
                removal_requested(&e)
            ),
            (
                "confirm_remove_this_device",
                vec![
                    "confirm_remove_this_device".to_string(),
                    "cancel_remove_this_device".to_string()
                ],
                true,
                None,
                true,
                Some(false)
            )
        );
    }

    // @internal
    #[test]
    fn on_the_only_active_device_removing_explains_instead_and_cancel_returns() {
        let mut e = alone();
        let _ = press(&mut e, "remove_this_device");
        let screen = e.current_screen();
        let ids = action_ids(&screen);
        let _ = press(&mut e, "cancel_remove_this_device");

        assert_eq!(
            (screen.screen_id.as_str(), ids, e.current_screen().screen_id),
            (
                "remove_this_device_only",
                vec!["cancel_remove_this_device".to_string()],
                "device_management".to_string()
            )
        );
    }

    // @internal
    #[test]
    fn when_the_other_devices_were_not_told_it_offers_retry_or_erase_anyway() {
        let mut e = with_a_tablet();
        let _ = press(&mut e, "remove_this_device");
        type_word(&mut e, "WIPE");
        let _ = press(&mut e, "confirm_remove_this_device");

        assert!(e.apply_update(crate::ui::EngineUpdate::DeviceRemovalNotTold));
        let screen = e.current_screen();
        let ids = action_ids(&screen);
        let retried = press(&mut e, "retry_remove_this_device");
        let retry_request = removal_requested(&e);
        let _ = e.apply_update(crate::ui::EngineUpdate::DeviceRemovalNotTold);
        let _ = press(&mut e, "erase_anyway_this_device");

        assert_eq!(
            (
                screen.screen_id.as_str(),
                ids,
                matches!(retried, ActionResult::Complete),
                retry_request,
                removal_requested(&e)
            ),
            (
                "remove_this_device_not_told",
                vec![
                    "retry_remove_this_device".to_string(),
                    "erase_anyway_this_device".to_string()
                ],
                true,
                Some(false),
                Some(true)
            )
        );
    }
}

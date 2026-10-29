// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Leave Vauchi asks, when the identity has other active devices, whether
//! they wipe too when the deletion runs (vauchi/private#916, maintainer
//! decisions 2026-10-29: asked when scheduling, carried with the schedule).

// INLINE_TEST_REQUIRED: Tests access the private GdprStep enum and engine state
#[cfg(test)]
mod tests {
    use super::super::*;

    fn engine(other_devices: &[&str], trusted: bool) -> GdprEngine {
        GdprEngine::new(None, "Active".into(), Locale::ENGLISH)
            .with_other_device_names(other_devices.iter().map(|n| (*n).to_string()).collect())
            .with_trusted_contacts(trusted)
    }

    fn press(e: &mut GdprEngine, id: &str) -> ActionResult {
        e.handle_action(UserAction::ActionPressed {
            action_id: id.into(),
        })
    }

    fn leave(e: &mut GdprEngine) -> ActionResult {
        let _ = press(e, "delete");
        press(e, "confirm_delete")
    }

    fn action_ids(screen: &ScreenModel) -> Vec<String> {
        screen
            .contextual_actions
            .iter()
            .map(|a| a.id.clone())
            .collect()
    }

    fn texts(screen: &ScreenModel) -> String {
        format!("{:?}", screen.components)
    }

    // @internal
    #[test]
    fn with_other_devices_leaving_asks_whether_they_wipe_and_names_them() {
        let mut e = engine(&["Laptop", "Tablet"], false);

        let _ = leave(&mut e);
        let screen = e.current_screen();

        assert_eq!(
            (
                screen.screen_id.as_str(),
                action_ids(&screen),
                texts(&screen).contains("Laptop") && texts(&screen).contains("Tablet")
            ),
            (
                "leave_wipe_other_devices",
                vec![
                    "wipe_other_devices".to_string(),
                    "keep_other_devices".to_string(),
                    "cancel".to_string()
                ],
                true
            )
        );
    }

    // @internal
    #[test]
    fn choosing_to_wipe_them_carries_the_choice_into_the_schedule() {
        let mut e = engine(&["Laptop"], false);
        let _ = leave(&mut e);

        let result = press(&mut e, "wipe_other_devices");

        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(GdprChoice::Delete {
                erase_guardian_set: true,
                wipe_other_devices: true,
            }))
        );
    }

    // @internal
    #[test]
    fn keeping_them_then_the_recovery_question_follows() {
        let mut e = engine(&["Laptop"], true);
        let _ = leave(&mut e);

        let _ = press(&mut e, "keep_other_devices");
        let next = e.current_screen().screen_id;
        let _ = press(&mut e, "keep_recovery");

        assert_eq!(
            (next.as_str(), e.engine_output()),
            (
                "delete_recovery_choice",
                Some(crate::ui::EngineOutput::Gdpr(GdprChoice::Delete {
                    erase_guardian_set: false,
                    wipe_other_devices: false,
                }))
            )
        );
    }

    // @internal
    #[test]
    fn on_the_only_device_leaving_does_not_ask_about_other_devices() {
        let mut e = engine(&[], false);

        let result = leave(&mut e);

        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(GdprChoice::Delete {
                erase_guardian_set: true,
                wipe_other_devices: false,
            }))
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! More menu engine — navigation hub for secondary screens.
//!
//! Displays a single `ActionList` with entries for screens that moved
//! out of the top-level tab bar: Sync, Devices, Settings, Backup,
//! Privacy, Help.  Each entry emits `OpenContact { contact_id }` where
//! the contact_id is a screen ID string — `route_result` in `routing.rs`
//! maps it to the target `AppScreen`.

use crate::ui::*;
use vauchi_core::{Command, FilePickPurpose};

/// `action_id` for the contacts-import entry. Special-cased in
/// `handle_action` to emit an `Command::FilePickFromUser`
/// rather than a navigation result — there is no "Import Contacts"
/// screen, the action *is* the picker.
pub(crate) const IMPORT_CONTACTS_ACTION_ID: &str = "import_contacts";

/// Navigation targets exposed through the More menu. The set is the
/// union of items every consuming platform (Android, TUI) needs to
/// reach from a "More" navigation surface. Android's MoreScreen
/// retirement (2026-05-01-more-engine-extension-android-retirement)
/// added the device-management / device-replacement / recovery /
/// archived-contacts / contact-duplicates entries; the legacy
/// activity-log / sync / backup / privacy entries stay for TUI.
///
/// `import_contacts` is the only entry that does not navigate to a
/// screen — selecting it returns an `Commands` result that
/// drives the frontend's native file picker per ADR-031.
const MORE_ITEMS: &[(&str, &str)] = &[
    ("activity_log", "Activity"),
    ("sync", "Sync"),
    ("device_management", "Linked Devices"),
    ("device_replacement", "Replace Device"),
    ("recovery", "Backup & Recovery"),
    ("archived_contacts", "Archived Contacts"),
    ("contact_duplicates", "Merge Contacts"),
    (IMPORT_CONTACTS_ACTION_ID, "Import Contacts"),
    ("settings", "Settings"),
    ("backup", "Backup"),
    ("privacy", "Privacy"),
    ("help", "Help"),
];

/// MIME types accepted for vCard import. Frontends may filter the
/// native picker to these; on platforms where the OS picker doesn't
/// filter by MIME (older Android variants), the frontend defaults to
/// `*/*` — the parser rejects non-vCard payloads anyway.
fn vcf_mime_types() -> Vec<String> {
    vec![
        "text/vcard".into(),
        "text/x-vcard".into(),
        "text/directory".into(),
    ]
}

/// Engine that renders a list of navigation targets for the "More" tab.
#[derive(Clone, Debug)]
pub struct MoreEngine;

impl Default for MoreEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MoreEngine {
    pub fn new() -> Self {
        Self
    }
}

impl WorkflowEngine for MoreEngine {
    fn current_screen(&self) -> ScreenModel {
        let items: Vec<ActionListItem> = MORE_ITEMS
            .iter()
            .map(|(id, label)| ActionListItem {
                id: (*id).into(),
                label: (*label).into(),
                icon: None,
                detail: None,
                a11y: None,
                info_key: None,
            })
            .collect();

        ScreenModel {
            screen_id: "more".into(),
            title: "More".into(),
            subtitle: None,
            components: vec![Component::ActionList {
                id: "more_menu".into(),
                items,
            }],
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id }
            | UserAction::ListItemSelected {
                item_id: action_id, ..
            } => {
                if action_id == IMPORT_CONTACTS_ACTION_ID {
                    // ADR-031: drive the native file picker via core,
                    // not via a navigation. The picker bytes flow back
                    // through `Event::FilePickedFromUser`
                    // and the AppEngine routes them to
                    // `Vauchi::import_contacts_from_vcf`.
                    return ActionResult::Commands {
                        commands: vec![Command::FilePickFromUser {
                            accepted_mime_types: vcf_mime_types(),
                            purpose: FilePickPurpose::ImportContacts,
                        }],
                    };
                }
                // Reuse OpenContact pattern — routing.rs maps the ID to an AppScreen.
                ActionResult::OpenContact {
                    contact_id: action_id,
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: tests verify the private MORE_ITEMS constant
// + the engine's special-case branch on the private
// IMPORT_CONTACTS_ACTION_ID — both are pub(crate)-or-tighter and not
// reachable from an external test crate.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn import_contacts_entry_is_present() {
        let entry = MORE_ITEMS
            .iter()
            .find(|(id, _)| *id == IMPORT_CONTACTS_ACTION_ID);
        assert!(
            entry.is_some(),
            "import_contacts entry missing from MORE_ITEMS"
        );
        assert_eq!(entry.unwrap().1, "Import Contacts");
    }

    // @internal
    #[test]
    fn import_contacts_action_emits_file_pick_command() {
        let mut engine = MoreEngine::new();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: IMPORT_CONTACTS_ACTION_ID.into(),
        });
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1, "expected exactly one command");
                match &commands[0] {
                    Command::FilePickFromUser {
                        accepted_mime_types,
                        purpose,
                    } => {
                        assert_eq!(*purpose, FilePickPurpose::ImportContacts);
                        assert!(
                            accepted_mime_types.iter().any(|m| m == "text/vcard"),
                            "expected text/vcard in accepted_mime_types, got {:?}",
                            accepted_mime_types
                        );
                    }
                    other => panic!("expected FilePickFromUser, got {:?}", other),
                }
            }
            other => panic!("expected Commands for import_contacts, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn import_contacts_action_via_list_item_selected_emits_file_pick_command() {
        // List-item selection from a `Component::ActionList` must
        // route the same as `ActionPressed` — both map to the same
        // affordance on every frontend.
        let mut engine = MoreEngine::new();
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "more_menu".into(),
            item_id: IMPORT_CONTACTS_ACTION_ID.into(),
        });
        assert!(
            matches!(result, ActionResult::Commands { .. }),
            "expected Commands for import_contacts via list-item, got {:?}",
            result
        );
    }

    // @internal
    #[test]
    fn other_more_entries_still_navigate_via_open_contact() {
        // Regression guard: only `import_contacts` should special-case;
        // every other entry must still emit `OpenContact` so the
        // existing routing.rs MoreScreen mapping continues to work.
        let mut engine = MoreEngine::new();
        for (id, _label) in MORE_ITEMS.iter() {
            if *id == IMPORT_CONTACTS_ACTION_ID {
                continue;
            }
            let result = engine.handle_action(UserAction::ActionPressed {
                action_id: (*id).into(),
            });
            match result {
                ActionResult::OpenContact { contact_id } => {
                    assert_eq!(contact_id, *id);
                }
                other => panic!("entry {} produced unexpected result: {:?}", id, other),
            }
        }
    }

    // @internal
    #[test]
    fn current_screen_includes_import_contacts_in_action_list() {
        let engine = MoreEngine::new();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "more");
        let action_list = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::ActionList { items, .. } => Some(items),
                _ => None,
            })
            .expect("ActionList component missing from More screen");
        assert!(
            action_list
                .iter()
                .any(|item| item.id == IMPORT_CONTACTS_ACTION_ID),
            "ActionList missing import_contacts entry"
        );
    }
}

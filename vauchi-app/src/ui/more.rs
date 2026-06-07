// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! More menu engine — navigation hub for secondary screens.
//!
//! Emits a `SectionedActionList` grouping entries into four sections
//! (primary / secondary / data / legal) per the shell-purity
//! investigation 2026-05-28-core-screen-composition-surface. Each
//! entry emits `OpenContact { contact_id }` where the contact_id is a
//! screen ID string — `route_result` in `routing.rs` maps it to the
//! target `AppScreen`. Section grouping mirrors iOS's existing
//! `MoreView` (`primarySection` / `secondarySection` / `legalSection`)
//! so the unified renderer adoption (G1 of
//! `2026-05-02-ios-humble-ui-deep-retirement`) is a like-for-like swap.

use crate::ui::*;
use vauchi_core::{Command, FilePickPurpose};

/// `action_id` for the contacts-import entry. Special-cased in
/// `handle_action` to emit an `Command::FilePickFromUser`
/// rather than a navigation result — there is no "Import Contacts"
/// screen, the action *is* the picker.
pub(crate) const IMPORT_CONTACTS_ACTION_ID: &str = "import_contacts";

/// Section in the More menu. Each section has a stable `id` (used by
/// the cross-platform contract pinned in
/// `tests/it/settings_more_parity_tests.rs`) and a `label` shown as
/// the section header on frontends that surface it.
struct MoreSection {
    id: &'static str,
    label: &'static str,
    items: &'static [(&'static str, &'static str)],
}

/// More menu sections — primary / secondary / data / legal.
///
/// The four-section grouping is the cross-platform contract:
/// - **primary**: most-used navigation (Settings, Help).
/// - **secondary**: device / sync / backup management.
/// - **data**: data-management entries (history, dedupe, import, archive).
/// - **legal**: policy disclosures.
///
/// Frontends with native section headers render the labels;
/// platforms without section affordances (TUI today) may flatten —
/// the iteration order across sections preserves the contracted
/// `EXPECTED_MORE_ACTION_IDS` sequence.
const MORE_SECTIONS: &[MoreSection] = &[
    MoreSection {
        id: "primary",
        label: "App",
        items: &[("settings", "Settings"), ("help", "Help")],
    },
    MoreSection {
        id: "secondary",
        label: "Account & Devices",
        items: &[
            ("sync", "Sync"),
            ("device_management", "Linked Devices"),
            ("device_replacement", "Replace Device"),
            // `recovery` opens the Social-Recovery screen — label it for
            // what it is, not "Backup & Recovery", which collided with the
            // adjacent file-`backup` entry and read as a duplicate.
            ("recovery", "Social Recovery"),
            ("backup", "Backup"),
        ],
    },
    MoreSection {
        id: "data",
        label: "Data",
        items: &[
            ("tags", "Tags"),
            ("archived_contacts", "Archived Contacts"),
            ("contact_duplicates", "Merge Contacts"),
            (IMPORT_CONTACTS_ACTION_ID, "Import Contacts"),
            ("activity_log", "Activity"),
        ],
    },
    MoreSection {
        id: "legal",
        label: "Legal",
        items: &[("privacy", "Privacy")],
    },
];

/// Iterate every `(action_id, label)` pair across all sections in
/// section order. Test-only — production code routes through the
/// emitted `Component::SectionedActionList` and its handler arms,
/// not through a flat helper.
#[cfg(test)]
fn iter_all_items() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    MORE_SECTIONS.iter().flat_map(|s| s.items.iter())
}

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
        let sections: Vec<Section> = MORE_SECTIONS
            .iter()
            .map(|sec| Section {
                id: sec.id.into(),
                label: sec.label.into(),
                items: sec
                    .items
                    .iter()
                    .map(|(id, label)| ActionListItem {
                        id: (*id).into(),
                        label: (*label).into(),
                        icon: None,
                        detail: None,
                        a11y: None,
                        info_key: None,
                    })
                    .collect(),
            })
            .collect();

        ScreenModel {
            screen_id: "more".into(),
            title: "More".into(),
            subtitle: None,
            components: vec![Component::SectionedActionList {
                id: "more_menu".into(),
                sections,
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

// INLINE_TEST_REQUIRED: tests verify the private MORE_SECTIONS constant
// + the engine's special-case branch on the private
// IMPORT_CONTACTS_ACTION_ID — both are pub(crate)-or-tighter and not
// reachable from an external test crate.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn import_contacts_entry_is_present() {
        let entry = iter_all_items().find(|(id, _)| *id == IMPORT_CONTACTS_ACTION_ID);
        assert!(
            entry.is_some(),
            "import_contacts entry missing from MORE_SECTIONS"
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
        // List-item selection from the emitted SectionedActionList
        // must route the same as ActionPressed — both map to the
        // same affordance on every frontend. Walker emits
        // ListItemSelected { component_id: <list id>, item_id: <item id> }.
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
        for (id, _label) in iter_all_items() {
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
    fn current_screen_emits_sectioned_action_list_with_import_contacts() {
        let engine = MoreEngine::new();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "more");
        let sections = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::SectionedActionList { sections, .. } => Some(sections),
                _ => None,
            })
            .expect("SectionedActionList component missing from More screen");
        let has_import_contacts = sections
            .iter()
            .flat_map(|sec| sec.items.iter())
            .any(|item| item.id == IMPORT_CONTACTS_ACTION_ID);
        assert!(
            has_import_contacts,
            "SectionedActionList missing import_contacts entry across all sections"
        );
    }

    // @internal
    #[test]
    fn sections_have_stable_ids_and_are_non_empty() {
        // Each section's id is part of the cross-platform contract
        // (frontends may key A11y / analytics on section.id). Pin the
        // four section ids in declaration order + assert no section
        // ships empty — an empty section is a UX bug (renders an
        // orphan header).
        let engine = MoreEngine::new();
        let screen = engine.current_screen();
        let sections = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::SectionedActionList { sections, .. } => Some(sections),
                _ => None,
            })
            .expect("SectionedActionList component missing");
        let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["primary", "secondary", "data", "legal"]);
        for sec in sections {
            assert!(!sec.items.is_empty(), "section {} is empty", sec.id);
        }
    }
}

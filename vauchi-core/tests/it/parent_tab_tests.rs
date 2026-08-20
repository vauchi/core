// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `AppScreen::parent_tab_for(layout)` and
//! `AppEngine::current_tab_id(layout)` — the §1D pure-renderer
//! remediation surface that lets frontends drop their per-app
//! `screen_id` → `parent_tab` map in favour of an exhaustive
//! core-side resolver.

use vauchi_app::ui::{AppScreen, FormDialogType, TabLayout};

fn cid(s: &str) -> String {
    s.to_string()
}

// @internal
#[test]
fn parameterized_contact_screens_resolve_to_contacts_on_both_layouts() {
    for screen in [
        AppScreen::ContactDetail {
            contact_id: cid("c"),
        },
        AppScreen::ContactEdit {
            contact_id: cid("c"),
        },
        AppScreen::ContactVisibility {
            contact_id: cid("c"),
        },
        AppScreen::VerifyFingerprint {
            contact_id: cid("c"),
        },
        AppScreen::ContactDuplicates,
        AppScreen::ContactLimit,
        AppScreen::ArchivedContacts,
    ] {
        assert_eq!(
            screen.parent_tab_for(TabLayout::Mobile),
            Some(AppScreen::Contacts),
            "mobile: {screen:?}"
        );
        assert_eq!(
            screen.parent_tab_for(TabLayout::Desktop),
            Some(AppScreen::Contacts),
            "desktop: {screen:?}"
        );
    }
}

// @internal
#[test]
fn entry_detail_and_avatar_editor_resolve_to_my_info() {
    for screen in [
        AppScreen::MyInfoEntryDetail { field_id: cid("f") },
        AppScreen::AvatarEditor,
    ] {
        assert_eq!(
            screen.parent_tab_for(TabLayout::Mobile),
            Some(AppScreen::MyInfo)
        );
        assert_eq!(
            screen.parent_tab_for(TabLayout::Desktop),
            Some(AppScreen::MyInfo)
        );
    }
}

// @internal
#[test]
fn group_detail_resolves_to_groups() {
    let screen = AppScreen::GroupDetail { group_id: cid("g") };
    assert_eq!(
        screen.parent_tab_for(TabLayout::Mobile),
        Some(AppScreen::Groups)
    );
    assert_eq!(
        screen.parent_tab_for(TabLayout::Desktop),
        Some(AppScreen::Groups)
    );
}

// @internal
#[test]
fn recovery_subscreens_resolve_to_recovery_on_both_layouts() {
    for screen in [AppScreen::RecoveryHelp, AppScreen::RecoveryClaimReview] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.parent_tab_for(layout),
                Some(AppScreen::Recovery),
                "{screen:?} on {layout:?}"
            );
        }
    }
}

// @internal
#[test]
fn device_link_subscreens_resolve_to_device_management_on_both_layouts() {
    for screen in [AppScreen::DeviceLinking, AppScreen::DeviceReplacement] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.parent_tab_for(layout),
                Some(AppScreen::DeviceManagement),
                "{screen:?} on {layout:?}"
            );
        }
    }
}

// @internal
#[test]
fn duress_and_emergency_shred_collapse_to_settings() {
    for screen in [AppScreen::DuressPin, AppScreen::EmergencyShred] {
        assert_eq!(
            screen.parent_tab_for(TabLayout::Desktop),
            Some(AppScreen::Settings)
        );
        assert_eq!(
            screen.parent_tab_for(TabLayout::Mobile),
            Some(AppScreen::Settings)
        );
    }
}

// @internal
#[test]
fn delivery_status_resolves_to_exchange_on_both_layouts() {
    assert_eq!(
        AppScreen::DeliveryStatus.parent_tab_for(TabLayout::Mobile),
        Some(AppScreen::Exchange)
    );
    assert_eq!(
        AppScreen::DeliveryStatus.parent_tab_for(TabLayout::Desktop),
        Some(AppScreen::Exchange)
    );
}

// @internal
#[test]
fn lock_and_form_dialog_have_no_parent_tab() {
    assert_eq!(AppScreen::Lock.parent_tab_for(TabLayout::Mobile), None);
    assert_eq!(AppScreen::Lock.parent_tab_for(TabLayout::Desktop), None);

    let dialog = AppScreen::FormDialog {
        dialog_type: FormDialogType::CreateGroup,
    };
    assert_eq!(dialog.parent_tab_for(TabLayout::Mobile), None);
    assert_eq!(dialog.parent_tab_for(TabLayout::Desktop), None);
}

// @internal
#[test]
fn top_level_mobile_tabs_resolve_to_themselves() {
    for screen in [
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::Groups,
        AppScreen::Onboarding,
    ] {
        assert_eq!(
            screen.clone().parent_tab_for(TabLayout::Mobile),
            Some(screen)
        );
    }
}

// @internal
#[test]
fn top_level_desktop_sidebar_items_resolve_to_themselves() {
    for screen in [
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::Groups,
        AppScreen::Settings,
        AppScreen::Recovery,
        AppScreen::DeviceManagement,
        AppScreen::Backup,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::Help,
        AppScreen::ActivityLog,
        AppScreen::Tags,
        AppScreen::Places,
        AppScreen::Onboarding,
    ] {
        assert_eq!(
            screen.clone().parent_tab_for(TabLayout::Desktop),
            Some(screen)
        );
    }
}

// @internal
#[test]
fn desktop_parent_ids_match_sidebar_items_set() {
    // Every parent_tab_for(Desktop) result, when Some, must yield a
    // screen_id present in `sidebar_items` so frontends can directly
    // use it for sidebar selection.
    let sidebar_ids: std::collections::HashSet<&'static str> = [
        "my_info",
        "contacts",
        "exchange",
        "groups",
        "settings",
        "recovery",
        "device_management",
        "backup",
        "privacy",
        "support",
        "help",
        "activity_log",
        "more",
        "onboarding",
    ]
    .into_iter()
    .collect();

    for screen in [
        AppScreen::ContactDetail {
            contact_id: cid("c"),
        },
        AppScreen::DuressPin,
        AppScreen::Settings,
        AppScreen::DeliveryStatus,
    ] {
        let parent = screen.parent_tab_for(TabLayout::Desktop).unwrap();
        assert!(
            sidebar_ids.contains(parent.screen_id()),
            "parent {} of {:?} not in sidebar_items set",
            parent.screen_id(),
            screen
        );
    }
}

// @internal
#[test]
fn mobile_parent_ids_match_tab_info_set() {
    // Every parent_tab_for(Mobile) result, when Some, must yield a
    // screen_id present in `tab_info` so frontends can directly use
    // it for bottom-tab selection.
    let mobile_tab_ids: std::collections::HashSet<&'static str> = [
        "my_info",
        "contacts",
        "exchange",
        "groups",
        "settings",
        "recovery",
        "device_management",
        "backup",
        "privacy",
        "support",
        "help",
        "activity_log",
        "tags",
        "places",
        "onboarding",
    ]
    .into_iter()
    .collect();

    for screen in [
        AppScreen::Settings,
        AppScreen::Recovery,
        AppScreen::Help,
        AppScreen::DuressPin,
        AppScreen::ContactDetail {
            contact_id: cid("c"),
        },
        AppScreen::DeliveryStatus,
    ] {
        let parent = screen.parent_tab_for(TabLayout::Mobile).unwrap();
        assert!(
            mobile_tab_ids.contains(parent.screen_id()),
            "parent {} of {:?} not in tab_info set",
            parent.screen_id(),
            screen
        );
    }
}

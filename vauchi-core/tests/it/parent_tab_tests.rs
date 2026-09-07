// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `AppScreen::parent_tab_for(layout)` and
//! `AppEngine::current_tab_id(layout)` — the §1D pure-renderer
//! remediation surface that lets frontends drop their per-app
//! `screen_id` → `parent_tab` map in favour of an exhaustive
//! core-side resolver.

use std::collections::HashSet;

use vauchi_app::ui::{AppEngine, AppScreen, FormDialogType, TabLayout};
use vauchi_core::api::Vauchi;

fn cid(s: &str) -> String {
    s.to_string()
}

/// The screen ids the navigation actually renders, read off the engine
/// rather than transcribed.
///
/// The two set-membership guards below used to carry a hardcoded list of
/// fourteen ids. That list stayed green when the nav was cut to five,
/// because it was a copy of what the nav used to offer — the guard could
/// not see the change it existed to catch. Reading the ids from
/// `primary_destinations` is what makes it able to fail.
fn rendered_destination_ids() -> HashSet<String> {
    let pre_identity = AppEngine::new(Vauchi::in_memory().unwrap());
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let post_identity = AppEngine::new(vauchi);

    pre_identity
        .primary_destinations()
        .into_iter()
        .chain(post_identity.primary_destinations())
        .map(|screen| screen.screen_id().to_string())
        .collect()
}

/// Screens worth probing: one per bucket of the resolver, so a mapping
/// that starts pointing at a screen the nav never renders is caught for
/// every bucket rather than only for contacts.
fn probe_screens() -> Vec<AppScreen> {
    vec![
        AppScreen::ContactDetail {
            contact_id: cid("c"),
        },
        AppScreen::ArchivedContacts,
        AppScreen::MyInfoEntryDetail { field_id: cid("f") },
        AppScreen::Groups,
        AppScreen::GroupDetail { group_id: cid("g") },
        AppScreen::Tags,
        AppScreen::Places,
        AppScreen::TagPromotion { tag_id: cid("t") },
        AppScreen::Settings,
        AppScreen::SettingsAdvanced,
        AppScreen::DuressPin,
        AppScreen::Recovery,
        AppScreen::RecoveryHelp,
        AppScreen::Backup,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::Help,
        AppScreen::ActivityLog,
        AppScreen::DeviceManagement,
        AppScreen::DeviceLinking,
        AppScreen::DeliveryStatus,
    ]
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

/// Groups, Tags and Places are reached from the Contacts screen's
/// secondary actions since the nav was cut to five destinations, so
/// Contacts is what stays selected while the user is inside one — and
/// `groups` is no longer a selectable destination at all.
// @internal
#[test]
fn contact_vocabulary_screens_resolve_to_contacts_on_both_layouts() {
    for screen in [
        AppScreen::Groups,
        AppScreen::GroupDetail { group_id: cid("g") },
        AppScreen::Tags,
        AppScreen::Places,
    ] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.parent_tab_for(layout),
                Some(AppScreen::Contacts),
                "{screen:?} on {layout:?}"
            );
        }
    }
}

/// Recovery and its sub-screens are reached from a Settings row, so
/// Settings is the selection — Recovery itself is no longer rendered.
// @internal
#[test]
fn recovery_subscreens_resolve_to_settings_on_both_layouts() {
    for screen in [
        AppScreen::Recovery,
        AppScreen::RecoveryHelp,
        AppScreen::RecoveryClaimReview,
    ] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.parent_tab_for(layout),
                Some(AppScreen::Settings),
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

/// The screens the nav stopped offering collapse onto whichever
/// destination routes to them, so the shell highlights the destination
/// the user actually came through.
// @internal
#[test]
fn demoted_screens_collapse_to_the_destination_that_reaches_them() {
    for screen in [
        AppScreen::Backup,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::Help,
        AppScreen::ActivityLog,
    ] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.parent_tab_for(layout),
                Some(AppScreen::Settings),
                "{screen:?} on {layout:?}"
            );
        }
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
fn top_level_destinations_resolve_to_themselves_on_both_layouts() {
    for screen in [
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::DeviceManagement,
        AppScreen::Settings,
        AppScreen::Onboarding,
    ] {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            assert_eq!(
                screen.clone().parent_tab_for(layout),
                Some(screen.clone()),
                "{screen:?} on {layout:?}"
            );
        }
    }
}

/// Every `parent_tab_for` result, when `Some`, must name a screen the nav
/// renders — a selection target the shell cannot find highlights nothing,
/// and reports nothing.
// @internal
#[test]
fn parent_ids_are_always_rendered_destinations() {
    let rendered = rendered_destination_ids();
    assert!(
        !rendered.is_empty(),
        "no rendered destinations, so this test proved nothing"
    );

    for screen in probe_screens() {
        for layout in [TabLayout::Desktop, TabLayout::Mobile] {
            let parent = screen
                .parent_tab_for(layout)
                .unwrap_or_else(|| panic!("{screen:?} selects no destination on {layout:?}"));
            assert!(
                rendered.contains(parent.screen_id()),
                "parent `{}` of {screen:?} on {layout:?} is not a rendered \
                 destination: {rendered:?}",
                parent.screen_id(),
            );
        }
    }
}

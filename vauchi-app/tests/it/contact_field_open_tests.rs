// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tapping a contact's field opens it outside the app (vauchi/private#709).
//!
//! Drives the same path a shell does: project the contact screen, reduce the
//! row's opaque activation, dispatch it, and read the link the engine asks the
//! shell to open.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, PreparedSurface, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::exchange::capability::{DeviceCapabilities, Platform};
use vauchi_core::{ActionSpec, Command, Event, ImportSource, PresentationNode, SurfaceId};

const ADDRESS: &str = "Bahnhofstrasse 1, 8001 Zürich";
const ENCODED: &str = "Bahnhofstrasse+1%2C+8001+Z%C3%BCrich";

struct ContactScreen {
    engine: AppEngine,
    surface_id: SurfaceId,
    prepared: PreparedSurface,
}

/// A contact holding one field, open on `platform`.
fn contact_screen(platform: Platform, field: ContactField) -> (ContactScreen, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let field_id = field.id().to_string();
    let mut card = ContactCard::new("Bob");
    card.add_field(field).unwrap();
    let contact =
        Contact::from_import("contact-bob".into(), card, ImportSource::VcardFile, None, 0);
    let contact_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        platform,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::ContactDetail { contact_id });
    let surface_id = SurfaceId::new("contact.test").unwrap();
    let prepared = PreparedSurface::from_screen(surface_id.clone(), 1, &engine.current_screen())
        .expect("the contact screen projects to a generic surface");
    (
        ContactScreen {
            engine,
            surface_id,
            prepared,
        },
        field_id,
    )
}

impl ContactScreen {
    fn field_row(&self, field_id: &str) -> (Option<ActionSpec>, Vec<ActionSpec>) {
        fn walk(
            nodes: &[PresentationNode],
            list_id: &str,
        ) -> Option<(Option<ActionSpec>, Vec<ActionSpec>)> {
            nodes.iter().find_map(|node| match node {
                PresentationNode::List { id, rows, .. } if id.as_str() == list_id => {
                    let row = rows.first().expect("a field list holds its field's row");
                    Some((row.activation.clone(), row.secondary_actions.clone()))
                }
                PresentationNode::Group { children, .. } => walk(children, list_id),
                _ => None,
            })
        }
        let Command::ReplaceSurface { surface } = self.prepared.command() else {
            panic!("the contact screen projects atomically");
        };
        walk(&surface.nodes, &format!("field_{field_id}"))
            .unwrap_or_else(|| panic!("no row for field `{field_id}`"))
    }

    /// The link the engine asks the shell to open when `action` is tapped.
    fn open(&mut self, action: &ActionSpec) -> String {
        let user_action: UserAction = self
            .prepared
            .reduce(Event::ActionActivated {
                surface_id: self.surface_id.clone(),
                interaction_id: action.interaction_id.clone(),
            })
            .expect("the row's action reduces to an engine action");
        match self.engine.handle_action(user_action) {
            ActionResult::OpenUrl { url } => url,
            other => panic!("`{}` must open a link, got {other:?}", action.label),
        }
    }
}

/// Tap the field's row: its label and the link it opens.
fn tap(platform: Platform, field: ContactField) -> (String, String) {
    let (mut screen, field_id) = contact_screen(platform, field);
    let activation = screen
        .field_row(&field_id)
        .0
        .expect("the field's row is tappable");
    let url = screen.open(&activation);
    (activation.label, url)
}

/// Every extra choice on the field's row: its label and the link it opens.
fn choices(platform: Platform, field: ContactField) -> Vec<(String, String)> {
    let (mut screen, field_id) = contact_screen(platform, field);
    let secondary = screen.field_row(&field_id).1;
    secondary
        .iter()
        .map(|action| (action.label.clone(), screen.open(action)))
        .collect()
}

fn address() -> ContactField {
    ContactField::new(FieldType::Address, "Home", ADDRESS, 0)
}

// @scenario: navigation :: Tap phone number to call
#[test]
fn tapping_a_phone_number_calls_it() {
    let phone = ContactField::new(FieldType::Phone, "Mobile", "+44 20 7946 0958", 0);
    assert_eq!(
        tap(Platform::Android, phone),
        ("Call".to_string(), "tel:+442079460958".to_string())
    );
}

// @scenario: navigation :: Tap email to compose
#[test]
fn tapping_an_email_address_composes_a_mail() {
    let email = ContactField::new(FieldType::Email, "Work", "carol@example.com", 0);
    assert_eq!(
        tap(Platform::Ios, email),
        (
            "Send email".to_string(),
            "mailto:carol@example.com".to_string()
        )
    );
}

// @scenario: navigation :: Tap URL to open in browser
#[test]
fn tapping_a_website_opens_it() {
    let website = ContactField::new(FieldType::Website, "Blog", "example.org", 0);
    assert_eq!(
        tap(Platform::Desktop, website),
        (
            "Open website".to_string(),
            "https://example.org".to_string()
        )
    );
}

// @scenario: navigation :: Tap social handle to open in app
#[test]
fn tapping_a_social_handle_opens_the_profile() {
    let social = ContactField::new(FieldType::Social, "Twitter", "@bobsmith", 0);
    assert_eq!(
        tap(Platform::Android, social),
        (
            "Open profile".to_string(),
            "https://twitter.com/bobsmith".to_string()
        )
    );
}

// @scenario: navigation :: Tap address to open in maps
#[test]
fn tapping_an_address_opens_the_systems_maps_app() {
    assert_eq!(
        tap(Platform::Android, address()),
        ("Show on map".to_string(), format!("geo:0,0?q={ENCODED}"))
    );
}

// @scenario: contact_actions :: Address opens Apple Maps on iOS
#[test]
fn on_ios_tapping_an_address_opens_apple_maps() {
    assert_eq!(
        tap(Platform::Ios, address()),
        (
            "Show on map".to_string(),
            format!("https://maps.apple.com/?q={ENCODED}")
        )
    );
}

// @scenario: contact_actions :: Address opens Google Maps on Android
#[test]
fn on_android_an_address_also_offers_google_maps_and_the_open_maps() {
    assert_eq!(
        choices(Platform::Android, address()),
        vec![
            (
                "Open in Google Maps".to_string(),
                format!("https://www.google.com/maps/search/?api=1&query={ENCODED}")
            ),
            (
                "Open in OpenStreetMap".to_string(),
                format!("https://www.openstreetmap.org/search?query={ENCODED}")
            ),
            (
                "Open in Organic Maps".to_string(),
                format!("om://search?query={ENCODED}")
            ),
        ]
    );
}

// @internal
#[test]
fn elsewhere_an_address_also_offers_the_open_maps() {
    for platform in [Platform::Ios, Platform::Desktop] {
        assert_eq!(
            choices(platform.clone(), address()),
            vec![
                (
                    "Open in OpenStreetMap".to_string(),
                    format!("https://www.openstreetmap.org/search?query={ENCODED}")
                ),
                (
                    "Open in Organic Maps".to_string(),
                    format!("om://search?query={ENCODED}")
                ),
            ],
            "{platform:?}"
        );
    }
}

// @internal
#[test]
fn a_field_with_nothing_to_open_is_not_tappable() {
    for field in [
        ContactField::new(FieldType::Custom, "Note", "likes tea", 0),
        ContactField::new(FieldType::Birthday, "Birthday", "1990-05-01", 0),
    ] {
        let (screen, field_id) = contact_screen(Platform::Android, field);
        let (activation, secondary) = screen.field_row(&field_id);
        assert!(activation.is_none(), "{field_id} has no link, so no tap");
        assert!(
            secondary.is_empty(),
            "{field_id} has no link, so no choices"
        );
    }
}

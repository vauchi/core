// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the screen catalog by navigating seeded `AppEngine`s to every
//! screen reachable without hardware or a peer and recording each
//! `initial_commands` batch.

pub mod seed;

use vauchi_app::ui::{
    AppEngine, AppScreen, FormDialogType, RenderContext, ScreenCatalogEntry, ScreenCatalogFixture,
};
use vauchi_core::exchange::capability::types::{BiometricType, DeviceCapabilities, Platform};
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::types::AudioCapability;
use vauchi_core::{Command, Event, PresentationNode, SurfaceId, Vauchi};

use seed::SeededWorld;

pub const LOCK_PASSWORD: &str = "correct horse battery staple";

/// What a phone shell declares at boot (the iOS `DeviceCapabilitiesPusher`
/// set): every in-person transport present, no USB data port.
pub fn phone_capabilities() -> DeviceCapabilities {
    DeviceCapabilities {
        has_nfc: true,
        has_ble: true,
        has_camera: true,
        audio: AudioCapability::Full,
        has_biometrics: true,
        biometric_type: Some(BiometricType::FaceId),
        has_secure_enclave: true,
        platform: Platform::Ios,
        has_accelerometer: true,
        has_internet: true,
        has_usb_port: false,
        device_name: Some("iPhone".to_owned()),
    }
}

/// Loads the bundled English strings plus the real German locale from the
/// `locales` checkout that sits next to core (CI: `.clone-locales`).
pub fn init_fixture_i18n() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let locales_dir = manifest_dir
        .ancestors()
        .map(|ancestor| ancestor.join("locales"))
        .find(|candidate| candidate.join("en.json").is_file())
        .expect("fixture tests require the bundled English locale");
    vauchi_app::i18n::init(&locales_dir).expect("load production locale strings");
    let german = std::fs::read(manifest_dir.join("../../locales/de.json"))
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    vauchi_app::i18n::load_locale_from_bytes("de", &german).expect("German locale parses");
}

/// The surface id a catalog entry is about: its `code_id` up to the
/// first `-`, which is the screen's `screen_id()`.
pub fn entry_surface_id(entry: &ScreenCatalogEntry) -> &str {
    entry
        .code_id
        .split_once('-')
        .map_or(entry.code_id.as_str(), |(screen_id, _)| screen_id)
}

/// The batch's `ReplaceSurface` for `surface_id`. A sub-screen batch also
/// replaces its parent surface (the responsive companion pane), so the
/// first replacement is not necessarily the screen itself.
pub fn replaced_surface<'a>(
    commands: &'a [Command],
    surface_id: &str,
) -> &'a vauchi_core::SurfaceSpec {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } if surface.surface_id.as_str() == surface_id => {
                Some(surface)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("catalog batch must replace surface {surface_id}"))
}

/// Every surface id the batch replaces, in emission order.
pub fn replaced_surface_ids(commands: &[Command]) -> Vec<&str> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str()),
            _ => None,
        })
        .collect()
}

/// The externally tagged serde variant name, so a new node variant never
/// needs a match arm here.
pub fn node_kind(node: &PresentationNode) -> String {
    match serde_json::to_value(node).expect("presentation node serializes") {
        serde_json::Value::String(unit_variant) => unit_variant,
        serde_json::Value::Object(tagged) => tagged
            .keys()
            .next()
            .cloned()
            .expect("externally tagged node"),
        other => panic!("unexpected node encoding: {other}"),
    }
}

pub fn node_kinds(nodes: &[PresentationNode]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|node| {
            let mut kinds = vec![node_kind(node)];
            if let PresentationNode::Group { children, .. } = node {
                kinds.extend(node_kinds(children));
            }
            kinds
        })
        .collect()
}

struct Recorder<'a> {
    engine: AppEngine,
    locale: &'static str,
    screens: &'a mut Vec<ScreenCatalogEntry>,
}

impl<'a> Recorder<'a> {
    fn new(
        vauchi: Vauchi,
        locale: &'static str,
        screens: &'a mut Vec<ScreenCatalogEntry>,
    ) -> Recorder<'a> {
        let mut engine = AppEngine::new(vauchi);
        // A shell pushes its hardware set right after boot (the TUI's
        // `set_device_capabilities` call, the iOS capabilities pusher);
        // without it every hardware-gated exchange mode reads as absent.
        engine.set_device_capabilities(phone_capabilities());
        engine.set_render_context(RenderContext {
            locale: Some(locale.to_owned()),
            theme_id: None,
        });
        // `AppEngine::new` built the boot screen under the default locale;
        // rebuild it under the pushed one, the way a shell's boot does.
        let boot_screen = engine.current_app_screen().clone();
        engine.set_initial_screen(boot_screen);
        Recorder {
            engine,
            locale,
            screens,
        }
    }

    /// Shells write `<code_id>.png`, so a non-English capture of a screen
    /// carries its locale in the code_id (`contacts-de`) rather than
    /// sharing the English stem.
    fn unique_code_id(&self, code_id: &str) -> String {
        if self.locale == "en" {
            code_id.to_owned()
        } else {
            format!("{code_id}-{}", self.locale)
        }
    }

    fn record_current(&mut self, code_id: &str) {
        let commands = self
            .engine
            .initial_commands()
            .unwrap_or_else(|error| panic!("{code_id}: initial commands: {error:?}"));
        let entry = ScreenCatalogEntry {
            code_id: self.unique_code_id(code_id),
            title: String::new(),
            locale: self.locale.to_owned(),
            commands,
        };
        let title = replaced_surface(&entry.commands, entry_surface_id(&entry))
            .title
            .clone();
        self.screens.push(ScreenCatalogEntry { title, ..entry });
    }

    fn record(&mut self, screen: AppScreen) {
        let code_id = screen.screen_id().to_owned();
        self.record_variant(screen, &code_id);
    }

    fn record_variant(&mut self, screen: AppScreen, code_id: &str) {
        self.engine.navigate_to(screen);
        self.record_current(code_id);
    }

    /// Activate the row carrying `icon_token` on `surface_id` the way a
    /// shell relays a tap: through the surface's minted interaction id.
    fn activate_row(&mut self, surface_id: &str, icon_token: &str) {
        let commands = self
            .engine
            .initial_commands()
            .unwrap_or_else(|error| panic!("{surface_id}: initial commands: {error:?}"));
        let interaction_id = replaced_surface(&commands, surface_id)
            .nodes
            .iter()
            .filter_map(|node| match node {
                PresentationNode::List { rows, .. } => Some(rows),
                _ => None,
            })
            .flatten()
            .find(|row| row.icon_token.as_deref() == Some(icon_token))
            .and_then(|row| row.activation.as_ref())
            .unwrap_or_else(|| panic!("{surface_id} has an activatable {icon_token} row"))
            .interaction_id
            .clone();
        self.engine
            .dispatch(Event::ActionActivated {
                surface_id: SurfaceId::new(surface_id).expect("surface id"),
                interaction_id,
            })
            .unwrap_or_else(|error| panic!("{surface_id}: activate {icon_token}: {error:?}"));
    }
}

fn record_seeded(world: SeededWorld, locale: &'static str, screens: &mut Vec<ScreenCatalogEntry>) {
    let verified = world.verified().id.clone();
    let unverified = world.unverified().id.clone();
    let merge_primary = &world.contacts[2];
    let merge_secondary = &world.contacts[3];
    let group_id = world.family_group_id.clone();
    let own_field_id = world.own_phone_field_id.clone();
    let contact_merge = AppScreen::ContactMerge {
        primary_name: merge_primary.name.to_owned(),
        primary_fields: merge_primary.field_labels.clone(),
        secondary_name: merge_secondary.name.to_owned(),
        secondary_fields: merge_secondary.field_labels.clone(),
    };

    let mut recorder = Recorder::new(world.vauchi, locale, screens);
    recorder.record_current("my_info");
    for screen in [
        AppScreen::Contacts,
        AppScreen::ContactDetail {
            contact_id: verified.clone(),
        },
        AppScreen::ContactEdit {
            contact_id: verified.clone(),
        },
        AppScreen::ContactVisibility {
            contact_id: verified.clone(),
        },
        AppScreen::VerifyFingerprint {
            contact_id: unverified,
        },
        AppScreen::Exchange,
        AppScreen::Settings,
        AppScreen::SettingsAdvanced,
        AppScreen::SettingsAppearance,
        AppScreen::SettingsAccessibility,
        AppScreen::Help,
        AppScreen::Backup,
        AppScreen::DeviceLinking,
        AppScreen::DeviceManagement,
        AppScreen::DuressPin,
        AppScreen::ChangePassword,
        AppScreen::DecoyContacts,
        AppScreen::EmergencyShred,
        AppScreen::DeliveryStatus,
        AppScreen::Recovery,
        AppScreen::RecoveryHelp,
        AppScreen::Groups,
        AppScreen::GroupDetail {
            group_id: group_id.clone(),
        },
        AppScreen::Tags,
        AppScreen::Places,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::MyInfoEntryDetail {
            field_id: own_field_id.clone(),
        },
        AppScreen::ContactDuplicates,
        contact_merge,
        AppScreen::ContactLimit,
        AppScreen::ActivityLog,
        AppScreen::ArchivedContacts,
        AppScreen::DeviceReplacement,
        AppScreen::AvatarEditor,
        AppScreen::RecoveryClaimReview,
        AppScreen::LinkExchange,
        AppScreen::NfcExchange,
        AppScreen::DirectTransport,
    ] {
        recorder.record(screen);
    }
    for (mode, variant) in [
        (ExchangeMode::Glance, "glance"),
        (ExchangeMode::Hover, "hover"),
    ] {
        recorder.record_variant(
            AppScreen::MultiStageExchange { mode },
            &format!("multi_stage_exchange-{variant}"),
        );
    }
    for (mode, variant) in [
        (ExchangeMode::Magic, "magic"),
        (ExchangeMode::Bump, "bump"),
        (ExchangeMode::Shake, "shake"),
    ] {
        recorder.record_variant(
            AppScreen::BleExchange { mode },
            &format!("ble_exchange-{variant}"),
        );
    }
    let dialogs = [
        (
            FormDialogType::AddField {
                available_groups: vec![(group_id.clone(), "Family".to_owned())],
            },
            "add_field",
        ),
        (
            FormDialogType::EditField {
                field_id: own_field_id,
                field_label: "mobile".to_owned(),
                current_value: "+44 20 7946 0958".to_owned(),
                current_note: Some("Work hours only".to_owned()),
            },
            "edit_field",
        ),
        (
            FormDialogType::EditName {
                current_name: seed::OWNER_NAME.to_owned(),
            },
            "edit_name",
        ),
        (
            FormDialogType::EditRelayUrl {
                current_url: "https://relay.example.org".to_owned(),
            },
            "edit_relay_url",
        ),
        (FormDialogType::CreateGroup, "create_group"),
        (
            FormDialogType::RenameGroup {
                group_id,
                current_name: "Family".to_owned(),
            },
            "rename_group",
        ),
    ];
    for (dialog_type, variant) in dialogs {
        recorder.record_variant(
            AppScreen::FormDialog { dialog_type },
            &format!("form_dialog-{variant}"),
        );
    }
}

fn record_empty(locale: &'static str, screens: &mut Vec<ScreenCatalogEntry>) {
    let mut recorder = Recorder::new(seed::identity_only(), locale, screens);
    recorder.record_current("my_info-empty");
    recorder.record_variant(AppScreen::Contacts, "contacts-empty");
    // The picker collapses every mode but the hero behind "Other ways to
    // connect"; the catalog shows the full offer, as the design canvas does.
    recorder.engine.navigate_to(AppScreen::Exchange);
    recorder.activate_row("exchange", "more");
    recorder.record_current("exchange-no_groups");
}

fn record_onboarding(locale: &'static str, screens: &mut Vec<ScreenCatalogEntry>) {
    let mut recorder = Recorder::new(
        Vauchi::in_memory().expect("in-memory Core"),
        locale,
        screens,
    );
    recorder.record_current("onboarding");
}

fn record_lock(locale: &'static str, screens: &mut Vec<ScreenCatalogEntry>) {
    let mut vauchi = seed::identity_only();
    vauchi
        .setup_app_password(LOCK_PASSWORD)
        .expect("app password");
    let mut recorder = Recorder::new(vauchi, locale, screens);
    recorder.record_current("lock");
}

fn record_german(screens: &mut Vec<ScreenCatalogEntry>) {
    record_onboarding("de", screens);
    let mut recorder = Recorder::new(seed::seeded_world().vauchi, "de", screens);
    recorder.record_current("my_info");
    for screen in [AppScreen::Contacts, AppScreen::Settings] {
        recorder.record(screen);
    }
}

/// Build the catalog from fresh seeded engines. Call
/// [`init_fixture_i18n`] first.
pub fn build_catalog() -> ScreenCatalogFixture {
    let mut screens = Vec::new();
    record_onboarding("en", &mut screens);
    record_empty("en", &mut screens);
    record_lock("en", &mut screens);
    record_seeded(seed::seeded_world(), "en", &mut screens);
    record_german(&mut screens);
    ScreenCatalogFixture {
        schema_version: 1,
        screens,
    }
}

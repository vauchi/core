// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Device-language steps (internationalization.feature): the shell pushes the
//! OS language tag in the render context, Core picks the language, and the
//! command stream is checked against the shipped `locales/` files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Once;

use cucumber::{given, then, when};
use vauchi_app::i18n::{self, Locale};
use vauchi_app::ui::{AppEngine, AppScreen, RenderContext};
use vauchi_core::Vauchi;

use crate::VauchiWorld;

/// A well-formed tag no `locales/` file names.
const UNSUPPORTED_LANGUAGE_TAG: &str = "sw-KE";

fn locales_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    assert!(
        dir.join("en.json").exists(),
        "no locales checkout at {} (CI: .clone-locales; locally: just setup)",
        dir.display()
    );
    dir
}

fn shipped_locale_codes() -> BTreeSet<String> {
    std::fs::read_dir(locales_dir())
        .expect("read locales dir")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let code = name.strip_suffix(".json")?;
            Locale::from_code(code)
                .filter(|locale| locale.code() == code)
                .map(|_| code.to_string())
        })
        .collect()
}

fn locale_file(code: &str) -> serde_json::Map<String, serde_json::Value> {
    let path = locales_dir().join(format!("{code}.json"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn locale_strings(code: &str) -> BTreeMap<String, String> {
    locale_file(code)
        .into_iter()
        .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
        .collect()
}

fn load_shipped_locales() {
    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        for code in shipped_locale_codes() {
            let path = locales_dir().join(format!("{code}.json"));
            let bytes = std::fs::read(&path).expect("read locale file");
            i18n::load_locale_from_bytes(&code, &bytes)
                .unwrap_or_else(|e| panic!("Core refused {code}.json: {e:?}"));
        }
    });
}

fn device_language(world: &mut VauchiWorld, tag: &str) {
    load_shipped_locales();
    world.system_locale = Some(tag.to_string());
}

/// Hosts the world's Vauchi in an engine whose render context carries the
/// device language, as a shell pushes it at boot.
pub fn open_engine(world: &mut VauchiWorld) {
    let vauchi = std::mem::replace(&mut world.vauchi, Vauchi::in_memory().unwrap());
    let mut engine = AppEngine::new(vauchi);
    if let Some(tag) = world.system_locale.clone() {
        engine.set_render_context(RenderContext {
            system_locale: Some(tag),
            ..RenderContext::default()
        });
    }
    world.engine = Some(engine);
}

fn engine(world: &mut VauchiWorld) -> &mut AppEngine {
    world
        .engine
        .as_mut()
        .expect("no AppEngine — open the app first")
}

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|i| collect_strings(i, out)),
        serde_json::Value::Object(map) => map.values().for_each(|i| collect_strings(i, out)),
        _ => {}
    }
}

fn presentation(engine: &mut AppEngine) -> serde_json::Value {
    let commands = engine.initial_commands().expect("compose presentation");
    serde_json::to_value(&commands).expect("commands serialize")
}

fn rendered_strings(world: &mut VauchiWorld) -> BTreeSet<String> {
    let mut out = Vec::new();
    collect_strings(&presentation(engine(world)), &mut out);
    out.into_iter().collect()
}

fn resolved_locale(world: &mut VauchiWorld) -> Locale {
    engine(world).render_context().resolved_locale()
}

/// English copy that the resolved language translates differently, so its
/// appearance on screen is a fallback, not a shared word.
fn english_only_strings(code: &str) -> BTreeSet<String> {
    let english = locale_strings("en");
    let target = locale_strings(code);
    let target_values: BTreeSet<&String> = target.values().collect();
    english
        .iter()
        .filter(|(key, value)| target.get(*key) != Some(*value) && !target_values.contains(value))
        .map(|(_, value)| value.clone())
        .collect()
}

fn own_strings(code: &str) -> BTreeSet<String> {
    let english: BTreeSet<String> = locale_strings("en").into_values().collect();
    locale_strings(code)
        .into_values()
        .filter(|value| code == "en" || !english.contains(value))
        .collect()
}

fn assert_renders_in(world: &mut VauchiWorld, expected: Locale) {
    assert_eq!(resolved_locale(world), expected, "resolved language");
    let shown = rendered_strings(world);
    let own: Vec<String> = shown
        .intersection(&own_strings(expected.code()))
        .cloned()
        .collect();
    assert!(
        !own.is_empty(),
        "no {} copy on the first screen; shown: {shown:?}",
        expected.code()
    );
}

// ── Given ───────────────────────────────────────────────────────────────────

#[given("my device is set to English")]
fn device_english(world: &mut VauchiWorld) {
    device_language(world, "en-US");
}

#[given("my device is set to German")]
fn device_german(world: &mut VauchiWorld) {
    device_language(world, "de-DE");
}

#[given("my device is set to French")]
fn device_french(world: &mut VauchiWorld) {
    device_language(world, "fr-FR");
}

#[given("my device is set to Spanish")]
fn device_spanish(world: &mut VauchiWorld) {
    device_language(world, "es-ES");
}

#[given("my device is set to Italian")]
fn device_italian(world: &mut VauchiWorld) {
    device_language(world, "it-IT");
}

#[given("my device is set to an unsupported language")]
fn device_unsupported(world: &mut VauchiWorld) {
    let code = UNSUPPORTED_LANGUAGE_TAG.split('-').next().unwrap();
    assert!(
        !shipped_locale_codes().contains(code),
        "{code}.json now ships; pick another unsupported tag"
    );
    device_language(world, UNSUPPORTED_LANGUAGE_TAG);
}

// ── When ────────────────────────────────────────────────────────────────────

#[when("I view the language selection screen")]
fn view_language_selection(world: &mut VauchiWorld) {
    load_shipped_locales();
    open_engine(world);
    engine(world).navigate_to(AppScreen::SettingsAppearance);
}

// ── Then ────────────────────────────────────────────────────────────────────

#[then("the app should display in German")]
fn display_in_german(world: &mut VauchiWorld) {
    assert_renders_in(world, Locale::GERMAN);
}

#[then("the app should display in English")]
fn display_in_english(world: &mut VauchiWorld) {
    assert_renders_in(world, Locale::ENGLISH);
}

#[then("all UI text should be translated")]
#[then("all UI strings should be translated")]
#[then("no English fallback text should appear")]
fn no_english_fallback(world: &mut VauchiWorld) {
    let code = resolved_locale(world).code().to_string();
    let shown = rendered_strings(world);
    let fallback: Vec<String> = shown
        .intersection(&english_only_strings(&code))
        .cloned()
        .collect();
    assert!(
        fallback.is_empty(),
        "English copy shown in {code}: {fallback:?}"
    );
}

#[then("the language should not need manual configuration")]
fn language_no_manual_config(world: &mut VauchiWorld) {
    let context = engine(world).render_context().clone();
    assert_eq!(context.locale, None, "a language was picked");
    let system = context.system_locale.as_deref().and_then(Locale::from_code);
    assert_eq!(Some(context.resolved_locale()), system);
}

#[then("all functionality should remain available")]
fn all_functionality_available(world: &mut VauchiWorld) {
    let shown = rendered_strings(world);
    let mut english = VauchiWorld::new();
    device_language(&mut english, "en-US");
    open_engine(&mut english);
    let in_english = rendered_strings(&mut english);
    assert_eq!(
        shown, in_english,
        "the fallback screen differs from the English one"
    );
}

/// The choice whose options are the shipped languages, from the commands.
fn language_choice(world: &mut VauchiWorld) -> serde_json::Map<String, serde_json::Value> {
    fn find(value: &serde_json::Value) -> Option<serde_json::Map<String, serde_json::Value>> {
        match value {
            serde_json::Value::Object(map) => {
                let offers_german =
                    map.get("options")
                        .and_then(|o| o.as_array())
                        .is_some_and(|options| {
                            options
                                .iter()
                                .any(|o| o.get("id").and_then(|i| i.as_str()) == Some("de"))
                        });
                if offers_german {
                    return Some(map.clone());
                }
                map.values().find_map(find)
            }
            serde_json::Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    let shown = presentation(engine(world));
    find(&shown).unwrap_or_else(|| panic!("no language choice on the screen: {shown}"))
}

fn options_of(choice: &serde_json::Map<String, serde_json::Value>) -> BTreeMap<String, String> {
    choice["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| {
            (
                o["id"].as_str().expect("option id").to_string(),
                o["label"].as_str().expect("option label").to_string(),
            )
        })
        .collect()
}

#[then("I should see a list of available languages")]
fn see_list_of_languages(world: &mut VauchiWorld) {
    let offered: BTreeSet<String> = options_of(&language_choice(world))
        .into_keys()
        .filter(|id| id != "follow_system")
        .collect();
    assert_eq!(offered, shipped_locale_codes(), "languages offered");
}

#[then("each language should be shown in its native script")]
fn language_in_native_script(world: &mut VauchiWorld) {
    for (code, label) in options_of(&language_choice(world)) {
        if code == "follow_system" {
            continue;
        }
        let native = locale_file(&code)["_meta"]["name"]
            .as_str()
            .unwrap_or_else(|| panic!("{code}.json has no _meta.name"))
            .to_string();
        assert_eq!(label, native, "label of {code}");
    }
}

#[then("the current language should be indicated")]
fn current_language_indicated(world: &mut VauchiWorld) {
    let choice = language_choice(world);
    let selected = choice
        .get("selected")
        .and_then(|s| s.as_str())
        .unwrap_or_else(|| panic!("the language choice selects nothing: {choice:?}"));
    assert_eq!(selected, "follow_system", "no language was picked");
    assert!(
        options_of(&choice).contains_key(selected),
        "the selected value {selected:?} is not an option"
    );
}

#[then("the translation should be complete")]
fn translation_complete(world: &mut VauchiWorld) {
    let locale = resolved_locale(world);
    let english = locale_strings("en");
    let target = locale_strings(locale.code());
    let missing: Vec<&String> = english
        .keys()
        .filter(|k| !target.contains_key(*k))
        .collect();
    assert!(missing.is_empty(), "{} lacks {missing:?}", locale.code());
    let served_otherwise: Vec<&String> = target
        .iter()
        .filter(|(key, value)| i18n::get_string(locale, key) != **value)
        .map(|(key, _)| key)
        .collect();
    assert!(
        served_otherwise.is_empty(),
        "Core does not serve the {} copy for {served_otherwise:?}",
        locale.code()
    );
}

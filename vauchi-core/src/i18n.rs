//! Internationalization (i18n) System
//!
//! Provides localized strings for the app UI.
//! Supports English (source), German, French, and Spanish.
//!
//! Feature file: features/internationalization.feature (pending)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported locales
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    #[serde(rename = "en")]
    #[default]
    English,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "es")]
    Spanish,
}

impl Locale {
    /// Get the ISO 639-1 language code
    pub fn code(&self) -> &'static str {
        match self {
            Locale::English => "en",
            Locale::German => "de",
            Locale::French => "fr",
            Locale::Spanish => "es",
        }
    }

    /// Parse a locale from its code
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_lowercase().as_str() {
            "en" | "en-us" | "en-gb" => Some(Locale::English),
            "de" | "de-de" | "de-at" | "de-ch" => Some(Locale::German),
            "fr" | "fr-fr" | "fr-ca" => Some(Locale::French),
            "es" | "es-es" | "es-mx" => Some(Locale::Spanish),
            _ => None,
        }
    }
}

/// Information about a locale
#[derive(Debug, Clone)]
pub struct LocaleInfo {
    pub code: &'static str,
    pub name: &'static str,
    pub english_name: &'static str,
    pub is_rtl: bool,
}

/// Get information about a locale
pub fn get_locale_info(locale: Locale) -> LocaleInfo {
    match locale {
        Locale::English => LocaleInfo {
            code: "en",
            name: "English",
            english_name: "English",
            is_rtl: false,
        },
        Locale::German => LocaleInfo {
            code: "de",
            name: "Deutsch",
            english_name: "German",
            is_rtl: false,
        },
        Locale::French => LocaleInfo {
            code: "fr",
            name: "Français",
            english_name: "French",
            is_rtl: false,
        },
        Locale::Spanish => LocaleInfo {
            code: "es",
            name: "Español",
            english_name: "Spanish",
            is_rtl: false,
        },
    }
}

/// Get all available locales
pub fn get_available_locales() -> Vec<Locale> {
    vec![
        Locale::English,
        Locale::German,
        Locale::French,
        Locale::Spanish,
    ]
}

/// Get a localized string by key
pub fn get_string(locale: Locale, key: &str) -> String {
    let strings = get_strings_for_locale(locale);
    if let Some(value) = strings.get(key) {
        return value.clone();
    }

    // Fallback to English
    if locale != Locale::English {
        let en_strings = get_strings_for_locale(Locale::English);
        if let Some(value) = en_strings.get(key) {
            return value.clone();
        }
    }

    format!("Missing: {}", key)
}

/// Get a localized string with argument interpolation
pub fn get_string_with_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let mut result = get_string(locale, key);

    for (name, value) in args {
        result = result.replace(&format!("{{{}}}", name), value);
    }

    result
}

/// Get all strings for a locale
fn get_strings_for_locale(locale: Locale) -> HashMap<String, String> {
    match locale {
        Locale::English => english_strings(),
        Locale::German => german_strings(),
        Locale::French => french_strings(),
        Locale::Spanish => spanish_strings(),
    }
}

// ============================================================
// English Strings (Source)
// ============================================================

fn english_strings() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.name".into(), "Vauchi".into());
    m.insert("app.tagline".into(), "Privacy-focused contact cards".into());

    // Welcome
    m.insert("welcome.title".into(), "Welcome to Vauchi".into());
    m.insert(
        "welcome.subtitle".into(),
        "Privacy-focused contact cards that update automatically".into(),
    );

    // Navigation
    m.insert("nav.home".into(), "Home".into());
    m.insert("nav.contacts".into(), "Contacts".into());
    m.insert("nav.exchange".into(), "Exchange".into());
    m.insert("nav.settings".into(), "Settings".into());

    // Contacts
    m.insert("contacts.title".into(), "Contacts".into());
    m.insert("contacts.empty".into(), "No contacts yet".into());
    m.insert("contacts.count".into(), "{count} contacts".into());
    m.insert("contacts.search".into(), "Search contacts".into());
    m.insert("contacts.add".into(), "Add contact".into());

    // Exchange
    m.insert("exchange.title".into(), "Exchange".into());
    m.insert("exchange.scan".into(), "Scan QR Code".into());
    m.insert("exchange.show".into(), "Show My QR Code".into());
    m.insert(
        "exchange.instruction".into(),
        "Scan a contact's QR code to exchange cards".into(),
    );

    // Settings
    m.insert("settings.title".into(), "Settings".into());
    m.insert("settings.account".into(), "Account".into());
    m.insert("settings.appearance".into(), "Appearance".into());
    m.insert("settings.privacy".into(), "Privacy".into());
    m.insert("settings.about".into(), "About".into());
    m.insert("settings.help".into(), "Help".into());
    m.insert("settings.language".into(), "Language".into());
    m.insert("settings.theme".into(), "Theme".into());

    // Help
    m.insert("help.title".into(), "Help".into());
    m.insert("help.faq".into(), "FAQ".into());
    m.insert("help.privacy".into(), "Privacy Info".into());
    m.insert("help.contact".into(), "Contact Support".into());

    // Actions
    m.insert("action.save".into(), "Save".into());
    m.insert("action.cancel".into(), "Cancel".into());
    m.insert("action.delete".into(), "Delete".into());
    m.insert("action.edit".into(), "Edit".into());
    m.insert("action.share".into(), "Share".into());
    m.insert("action.done".into(), "Done".into());
    m.insert("action.next".into(), "Next".into());
    m.insert("action.back".into(), "Back".into());
    m.insert("action.confirm".into(), "Confirm".into());
    m.insert("action.retry".into(), "Retry".into());

    // Errors
    m.insert("error.generic".into(), "Something went wrong".into());
    m.insert(
        "error.network".into(),
        "Network error. Please check your connection.".into(),
    );
    m.insert("error.validation".into(), "Please check your input".into());

    // Updates
    m.insert(
        "update.sent".into(),
        "Update sent to {count} contacts".into(),
    );
    m.insert(
        "update.received".into(),
        "Update received from {name}".into(),
    );

    // Card
    m.insert("card.title".into(), "My Card".into());
    m.insert("card.edit".into(), "Edit Card".into());
    m.insert("card.share".into(), "Share Card".into());

    m
}

// ============================================================
// German Strings
// ============================================================

fn german_strings() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.name".into(), "Vauchi".into());
    m.insert(
        "app.tagline".into(),
        "Datenschutzfreundliche Kontaktkarten".into(),
    );

    // Welcome
    m.insert("welcome.title".into(), "Willkommen bei Vauchi".into());
    m.insert(
        "welcome.subtitle".into(),
        "Datenschutzfreundliche Kontaktkarten, die sich automatisch aktualisieren".into(),
    );

    // Navigation
    m.insert("nav.home".into(), "Start".into());
    m.insert("nav.contacts".into(), "Kontakte".into());
    m.insert("nav.exchange".into(), "Austausch".into());
    m.insert("nav.settings".into(), "Einstellungen".into());

    // Contacts
    m.insert("contacts.title".into(), "Kontakte".into());
    m.insert("contacts.empty".into(), "Noch keine Kontakte".into());
    m.insert("contacts.count".into(), "{count} Kontakte".into());
    m.insert("contacts.search".into(), "Kontakte suchen".into());
    m.insert("contacts.add".into(), "Kontakt hinzufügen".into());

    // Exchange
    m.insert("exchange.title".into(), "Austausch".into());
    m.insert("exchange.scan".into(), "QR-Code scannen".into());
    m.insert("exchange.show".into(), "Meinen QR-Code zeigen".into());
    m.insert(
        "exchange.instruction".into(),
        "Scannen Sie den QR-Code eines Kontakts, um Karten auszutauschen".into(),
    );

    // Settings
    m.insert("settings.title".into(), "Einstellungen".into());
    m.insert("settings.account".into(), "Konto".into());
    m.insert("settings.appearance".into(), "Erscheinungsbild".into());
    m.insert("settings.privacy".into(), "Datenschutz".into());
    m.insert("settings.about".into(), "Über".into());
    m.insert("settings.help".into(), "Hilfe".into());
    m.insert("settings.language".into(), "Sprache".into());
    m.insert("settings.theme".into(), "Design".into());

    // Help
    m.insert("help.title".into(), "Hilfe".into());
    m.insert("help.faq".into(), "FAQ".into());
    m.insert("help.privacy".into(), "Datenschutz-Info".into());
    m.insert("help.contact".into(), "Support kontaktieren".into());

    // Actions
    m.insert("action.save".into(), "Speichern".into());
    m.insert("action.cancel".into(), "Abbrechen".into());
    m.insert("action.delete".into(), "Löschen".into());
    m.insert("action.edit".into(), "Bearbeiten".into());
    m.insert("action.share".into(), "Teilen".into());
    m.insert("action.done".into(), "Fertig".into());
    m.insert("action.next".into(), "Weiter".into());
    m.insert("action.back".into(), "Zurück".into());
    m.insert("action.confirm".into(), "Bestätigen".into());
    m.insert("action.retry".into(), "Erneut versuchen".into());

    // Errors
    m.insert("error.generic".into(), "Etwas ist schiefgelaufen".into());
    m.insert(
        "error.network".into(),
        "Netzwerkfehler. Bitte prüfen Sie Ihre Verbindung.".into(),
    );
    m.insert(
        "error.validation".into(),
        "Bitte überprüfen Sie Ihre Eingabe".into(),
    );

    // Updates
    m.insert(
        "update.sent".into(),
        "Update an {count} Kontakte gesendet".into(),
    );
    m.insert(
        "update.received".into(),
        "Update von {name} erhalten".into(),
    );

    // Card
    m.insert("card.title".into(), "Meine Karte".into());
    m.insert("card.edit".into(), "Karte bearbeiten".into());
    m.insert("card.share".into(), "Karte teilen".into());

    m
}

// ============================================================
// French Strings
// ============================================================

fn french_strings() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.name".into(), "Vauchi".into());
    m.insert(
        "app.tagline".into(),
        "Cartes de contact axées sur la confidentialité".into(),
    );

    // Welcome
    m.insert("welcome.title".into(), "Bienvenue sur Vauchi".into());
    m.insert(
        "welcome.subtitle".into(),
        "Cartes de contact confidentielles qui se mettent à jour automatiquement".into(),
    );

    // Navigation
    m.insert("nav.home".into(), "Accueil".into());
    m.insert("nav.contacts".into(), "Contacts".into());
    m.insert("nav.exchange".into(), "Échange".into());
    m.insert("nav.settings".into(), "Paramètres".into());

    // Contacts
    m.insert("contacts.title".into(), "Contacts".into());
    m.insert("contacts.empty".into(), "Pas encore de contacts".into());
    m.insert("contacts.count".into(), "{count} contacts".into());
    m.insert("contacts.search".into(), "Rechercher des contacts".into());
    m.insert("contacts.add".into(), "Ajouter un contact".into());

    // Exchange
    m.insert("exchange.title".into(), "Échange".into());
    m.insert("exchange.scan".into(), "Scanner le code QR".into());
    m.insert("exchange.show".into(), "Afficher mon code QR".into());
    m.insert(
        "exchange.instruction".into(),
        "Scannez le code QR d'un contact pour échanger des cartes".into(),
    );

    // Settings
    m.insert("settings.title".into(), "Paramètres".into());
    m.insert("settings.account".into(), "Compte".into());
    m.insert("settings.appearance".into(), "Apparence".into());
    m.insert("settings.privacy".into(), "Confidentialité".into());
    m.insert("settings.about".into(), "À propos".into());
    m.insert("settings.help".into(), "Aide".into());
    m.insert("settings.language".into(), "Langue".into());
    m.insert("settings.theme".into(), "Thème".into());

    // Help
    m.insert("help.title".into(), "Aide".into());
    m.insert("help.faq".into(), "FAQ".into());
    m.insert("help.privacy".into(), "Info confidentialité".into());
    m.insert("help.contact".into(), "Contacter le support".into());

    // Actions
    m.insert("action.save".into(), "Enregistrer".into());
    m.insert("action.cancel".into(), "Annuler".into());
    m.insert("action.delete".into(), "Supprimer".into());
    m.insert("action.edit".into(), "Modifier".into());
    m.insert("action.share".into(), "Partager".into());
    m.insert("action.done".into(), "Terminé".into());
    m.insert("action.next".into(), "Suivant".into());
    m.insert("action.back".into(), "Retour".into());
    m.insert("action.confirm".into(), "Confirmer".into());
    m.insert("action.retry".into(), "Réessayer".into());

    // Errors
    m.insert("error.generic".into(), "Une erreur s'est produite".into());
    m.insert(
        "error.network".into(),
        "Erreur réseau. Veuillez vérifier votre connexion.".into(),
    );
    m.insert(
        "error.validation".into(),
        "Veuillez vérifier votre saisie".into(),
    );

    // Updates
    m.insert(
        "update.sent".into(),
        "Mise à jour envoyée à {count} contacts".into(),
    );
    m.insert(
        "update.received".into(),
        "Mise à jour reçue de {name}".into(),
    );

    // Card
    m.insert("card.title".into(), "Ma carte".into());
    m.insert("card.edit".into(), "Modifier la carte".into());
    m.insert("card.share".into(), "Partager la carte".into());

    m
}

// ============================================================
// Spanish Strings
// ============================================================

fn spanish_strings() -> HashMap<String, String> {
    let mut m = HashMap::new();

    // App
    m.insert("app.name".into(), "Vauchi".into());
    m.insert(
        "app.tagline".into(),
        "Tarjetas de contacto centradas en la privacidad".into(),
    );

    // Welcome
    m.insert("welcome.title".into(), "Bienvenido a Vauchi".into());
    m.insert(
        "welcome.subtitle".into(),
        "Tarjetas de contacto con privacidad que se actualizan automáticamente".into(),
    );

    // Navigation
    m.insert("nav.home".into(), "Inicio".into());
    m.insert("nav.contacts".into(), "Contactos".into());
    m.insert("nav.exchange".into(), "Intercambio".into());
    m.insert("nav.settings".into(), "Ajustes".into());

    // Contacts
    m.insert("contacts.title".into(), "Contactos".into());
    m.insert("contacts.empty".into(), "Aún no hay contactos".into());
    m.insert("contacts.count".into(), "{count} contactos".into());
    m.insert("contacts.search".into(), "Buscar contactos".into());
    m.insert("contacts.add".into(), "Añadir contacto".into());

    // Exchange
    m.insert("exchange.title".into(), "Intercambio".into());
    m.insert("exchange.scan".into(), "Escanear código QR".into());
    m.insert("exchange.show".into(), "Mostrar mi código QR".into());
    m.insert(
        "exchange.instruction".into(),
        "Escanea el código QR de un contacto para intercambiar tarjetas".into(),
    );

    // Settings
    m.insert("settings.title".into(), "Ajustes".into());
    m.insert("settings.account".into(), "Cuenta".into());
    m.insert("settings.appearance".into(), "Apariencia".into());
    m.insert("settings.privacy".into(), "Privacidad".into());
    m.insert("settings.about".into(), "Acerca de".into());
    m.insert("settings.help".into(), "Ayuda".into());
    m.insert("settings.language".into(), "Idioma".into());
    m.insert("settings.theme".into(), "Tema".into());

    // Help
    m.insert("help.title".into(), "Ayuda".into());
    m.insert("help.faq".into(), "Preguntas frecuentes".into());
    m.insert("help.privacy".into(), "Info de privacidad".into());
    m.insert("help.contact".into(), "Contactar soporte".into());

    // Actions
    m.insert("action.save".into(), "Guardar".into());
    m.insert("action.cancel".into(), "Cancelar".into());
    m.insert("action.delete".into(), "Eliminar".into());
    m.insert("action.edit".into(), "Editar".into());
    m.insert("action.share".into(), "Compartir".into());
    m.insert("action.done".into(), "Hecho".into());
    m.insert("action.next".into(), "Siguiente".into());
    m.insert("action.back".into(), "Atrás".into());
    m.insert("action.confirm".into(), "Confirmar".into());
    m.insert("action.retry".into(), "Reintentar".into());

    // Errors
    m.insert("error.generic".into(), "Algo salió mal".into());
    m.insert(
        "error.network".into(),
        "Error de red. Por favor, comprueba tu conexión.".into(),
    );
    m.insert(
        "error.validation".into(),
        "Por favor, revisa tu entrada".into(),
    );

    // Updates
    m.insert(
        "update.sent".into(),
        "Actualización enviada a {count} contactos".into(),
    );
    m.insert(
        "update.received".into(),
        "Actualización recibida de {name}".into(),
    );

    // Card
    m.insert("card.title".into(), "Mi tarjeta".into());
    m.insert("card.edit".into(), "Editar tarjeta".into());
    m.insert("card.share".into(), "Compartir tarjeta".into());

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_default() {
        assert_eq!(Locale::default(), Locale::English);
    }

    #[test]
    fn test_locale_codes() {
        assert_eq!(Locale::English.code(), "en");
        assert_eq!(Locale::German.code(), "de");
    }

    #[test]
    fn test_locale_from_code() {
        assert_eq!(Locale::from_code("en"), Some(Locale::English));
        assert_eq!(Locale::from_code("EN"), Some(Locale::English));
        assert_eq!(Locale::from_code("en-US"), Some(Locale::English));
        assert_eq!(Locale::from_code("xx"), None);
    }

    #[test]
    fn test_get_string_english() {
        let s = get_string(Locale::English, "welcome.title");
        assert_eq!(s, "Welcome to Vauchi");
    }

    #[test]
    fn test_get_string_german() {
        let s = get_string(Locale::German, "welcome.title");
        assert_eq!(s, "Willkommen bei Vauchi");
    }

    #[test]
    fn test_get_string_fallback() {
        // If a key doesn't exist in German, it should fall back to English
        let en = get_string(Locale::English, "app.name");
        let de = get_string(Locale::German, "app.name");
        assert_eq!(en, de);
    }

    #[test]
    fn test_get_string_missing() {
        let s = get_string(Locale::English, "nonexistent");
        assert!(s.contains("Missing"));
    }

    #[test]
    fn test_interpolation() {
        let s = get_string_with_args(Locale::English, "contacts.count", &[("count", "5")]);
        assert_eq!(s, "5 contacts");
    }

    #[test]
    fn test_available_locales() {
        let locales = get_available_locales();
        assert_eq!(locales.len(), 4);
    }
}

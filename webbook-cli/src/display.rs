//! Display Helpers
//!
//! Terminal output formatting and styling.

use console::{style, Style};
use webbook_core::{ContactCard, Contact, FieldType};

/// Prints a success message.
pub fn success(msg: &str) {
    println!("{} {}", style("✓").green().bold(), msg);
}

/// Prints an error message.
pub fn error(msg: &str) {
    eprintln!("{} {}", style("✗").red().bold(), msg);
}

/// Prints a warning message.
pub fn warning(msg: &str) {
    println!("{} {}", style("⚠").yellow().bold(), msg);
}

/// Prints an info message.
pub fn info(msg: &str) {
    println!("{} {}", style("ℹ").blue().bold(), msg);
}

/// Returns the icon for a field type.
fn field_icon(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::Email => "mail",
        FieldType::Phone => "phone",
        FieldType::Website => "web",
        FieldType::Address => "home",
        FieldType::Social => "share",
        FieldType::Custom => "note",
    }
}

/// Displays a contact card in a formatted box.
pub fn display_card(card: &ContactCard) {
    let name = card.display_name();
    let width = 40;

    // Top border
    println!("{}", "─".repeat(width));

    // Name
    println!("  {}", style(name).bold().cyan());

    // Separator
    println!("{}", "─".repeat(width));

    // Fields
    if card.fields().is_empty() {
        println!("  {}", style("(no fields)").dim());
    } else {
        for field in card.fields() {
            let icon = field_icon(field.field_type());
            let label_style = Style::new().dim();
            println!(
                "  {:6} {:12} {}",
                icon,
                label_style.apply_to(field.label()),
                field.value()
            );
        }
    }

    // Bottom border
    println!("{}", "─".repeat(width));
}

/// Displays a contact in a compact format.
pub fn display_contact_summary(contact: &Contact, index: usize) {
    let name = contact.display_name();
    let verified = if contact.is_fingerprint_verified() {
        style("✓ verified").green()
    } else {
        style("").dim()
    };

    println!("  {}. {}  {}", index, style(name).bold(), verified);
}

/// Displays a contact with full details.
pub fn display_contact_details(contact: &Contact) {
    let name = contact.display_name();
    let id = contact.id();

    println!();
    println!("  {}", style(name).bold().cyan());
    println!("  ID: {}", style(id).dim());

    if contact.is_fingerprint_verified() {
        println!("  Status: {}", style("Fingerprint verified").green());
    } else {
        println!("  Status: {}", style("Not verified").yellow());
    }

    println!();

    // Show card fields
    let card = contact.card();
    if card.fields().is_empty() {
        println!("  {}", style("(no visible fields)").dim());
    } else {
        for field in card.fields() {
            let icon = field_icon(field.field_type());
            println!(
                "  {:6} {:12} {}",
                icon,
                style(field.label()).dim(),
                field.value()
            );
        }
    }

    println!();
}

/// Displays a QR code in the terminal using Unicode blocks.
pub fn display_qr_code(data: &str) {
    use qrcode::QrCode;
    use qrcode::render::unicode;

    match QrCode::new(data) {
        Ok(code) => {
            let image = code.render::<unicode::Dense1x2>()
                .dark_color(unicode::Dense1x2::Light)
                .light_color(unicode::Dense1x2::Dark)
                .build();
            println!("{}", image);
        }
        Err(e) => {
            error(&format!("Failed to generate QR code: {}", e));
        }
    }
}

/// Displays exchange data for sharing.
pub fn display_exchange_data(data: &str) {
    println!();
    println!("Scan this QR code with another WebBook user:");
    println!();
    display_qr_code(data);
    println!();
    println!("Or share this text:");
    println!("{}", style(data).cyan());
    println!();
}

//! WebBook CLI
//!
//! Command-line interface for WebBook - privacy-focused contact card exchange.

mod commands;
mod config;
mod display;
mod protocol;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use config::CliConfig;

#[derive(Parser)]
#[command(name = "webbook")]
#[command(version, about = "Privacy-focused contact card exchange")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Data directory (default: ~/.webbook)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Relay server URL
    #[arg(long, global = true, default_value = "ws://localhost:8080")]
    relay: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new identity
    Init {
        /// Your display name
        name: String,
    },

    /// Manage your contact card
    #[command(subcommand)]
    Card(CardCommands),

    /// Exchange contacts with another user
    #[command(subcommand)]
    Exchange(ExchangeCommands),

    /// Manage your contacts
    #[command(subcommand)]
    Contacts(ContactCommands),

    /// Sync with the relay server
    Sync,

    /// Export identity backup
    Export {
        /// Output file path
        output: PathBuf,
    },

    /// Import identity from backup
    Import {
        /// Input file path
        input: PathBuf,
    },
}

#[derive(Subcommand)]
enum CardCommands {
    /// Show your contact card
    Show,

    /// Add a field to your card
    Add {
        /// Field type (email, phone, website, address, social, other)
        #[arg(value_name = "TYPE")]
        field_type: String,

        /// Field label (e.g., "work", "personal", "mobile")
        label: String,

        /// Field value
        value: String,
    },

    /// Remove a field from your card
    Remove {
        /// Field label to remove
        label: String,
    },

    /// Edit a field value
    Edit {
        /// Field label to edit
        label: String,

        /// New value
        value: String,
    },
}

#[derive(Subcommand)]
enum ExchangeCommands {
    /// Generate QR code for contact exchange
    Start,

    /// Complete exchange with another user's data
    Complete {
        /// Exchange data (wb:// URL or base64)
        data: String,
    },
}

#[derive(Subcommand)]
enum ContactCommands {
    /// List all contacts
    List,

    /// Show contact details
    Show {
        /// Contact ID or name
        id: String,
    },

    /// Search contacts by name
    Search {
        /// Search query
        query: String,
    },

    /// Remove a contact
    Remove {
        /// Contact ID
        id: String,
    },

    /// Mark contact fingerprint as verified
    Verify {
        /// Contact ID
        id: String,
    },

    /// Hide a field from a contact
    Hide {
        /// Contact ID or name
        contact: String,
        /// Field label to hide
        field: String,
    },

    /// Show a field to a contact (make visible)
    Unhide {
        /// Contact ID or name
        contact: String,
        /// Field label to unhide
        field: String,
    },

    /// Show visibility rules for a contact
    Visibility {
        /// Contact ID or name
        contact: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolve data directory
    let data_dir = cli.data_dir.unwrap_or_else(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("webbook")
    });

    let config = CliConfig {
        data_dir,
        relay_url: cli.relay,
    };

    match cli.command {
        Commands::Init { name } => {
            commands::init::run(&name, &config)?;
        }
        Commands::Card(cmd) => match cmd {
            CardCommands::Show => commands::card::show(&config)?,
            CardCommands::Add { field_type, label, value } => {
                commands::card::add(&config, &field_type, &label, &value)?;
            }
            CardCommands::Remove { label } => {
                commands::card::remove(&config, &label)?;
            }
            CardCommands::Edit { label, value } => {
                commands::card::edit(&config, &label, &value)?;
            }
        },
        Commands::Exchange(cmd) => match cmd {
            ExchangeCommands::Start => commands::exchange::start(&config)?,
            ExchangeCommands::Complete { data } => {
                commands::exchange::complete(&config, &data)?;
            }
        },
        Commands::Contacts(cmd) => match cmd {
            ContactCommands::List => commands::contacts::list(&config)?,
            ContactCommands::Show { id } => commands::contacts::show(&config, &id)?,
            ContactCommands::Search { query } => commands::contacts::search(&config, &query)?,
            ContactCommands::Remove { id } => commands::contacts::remove(&config, &id)?,
            ContactCommands::Verify { id } => commands::contacts::verify(&config, &id)?,
            ContactCommands::Hide { contact, field } => {
                commands::contacts::hide_field(&config, &contact, &field)?;
            }
            ContactCommands::Unhide { contact, field } => {
                commands::contacts::unhide_field(&config, &contact, &field)?;
            }
            ContactCommands::Visibility { contact } => {
                commands::contacts::show_visibility(&config, &contact)?;
            }
        },
        Commands::Sync => {
            commands::sync::run(&config).await?;
        }
        Commands::Export { output } => {
            commands::backup::export(&config, &output)?;
        }
        Commands::Import { input } => {
            commands::backup::import(&config, &input)?;
        }
    }

    Ok(())
}

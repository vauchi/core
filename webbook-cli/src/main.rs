//! WebBook CLI
//!
//! Command-line interface for WebBook - privacy-focused contact card exchange.

mod commands;
mod config;
mod display;
mod protocol;

use std::path::PathBuf;

use std::io;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

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

    /// Social network utilities
    #[command(subcommand)]
    Social(SocialCommands),

    /// Manage linked devices
    #[command(subcommand)]
    Device(DeviceCommands),

    /// Contact recovery via social vouching
    #[command(subcommand)]
    Recovery(RecoveryCommands),

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

    /// Generate shell completions
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: Shell,
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

#[derive(Subcommand)]
enum SocialCommands {
    /// List available social networks
    List {
        /// Optional search query
        query: Option<String>,
    },

    /// Get profile URL for a social network
    Url {
        /// Social network (e.g., twitter, github)
        network: String,
        /// Username on that network
        username: String,
    },
}

#[derive(Subcommand)]
enum DeviceCommands {
    /// List all linked devices
    List,

    /// Show info about the current device
    Info,

    /// Generate QR code to link a new device
    Link,

    /// Join an existing identity (on new device)
    Join {
        /// QR data from existing device
        qr_data: String,
    },

    /// Complete device linking (on existing device)
    Complete {
        /// Request data from new device
        request: String,
    },

    /// Finish device join (on new device)
    Finish {
        /// Response data from existing device
        response: String,
    },

    /// Revoke a linked device
    Revoke {
        /// Device ID prefix
        device_id: String,
    },
}

#[derive(Subcommand)]
enum RecoveryCommands {
    /// Create a recovery claim for a lost identity
    Claim {
        /// Old public key (hex) from lost device
        old_pk: String,
    },

    /// Vouch for someone's recovery claim
    Vouch {
        /// Recovery claim data (base64)
        claim: String,
    },

    /// Add a voucher to your recovery proof
    AddVoucher {
        /// Voucher data (base64)
        voucher: String,
    },

    /// Show recovery status
    Status,

    /// Show completed recovery proof
    Proof,

    /// Verify a recovery proof from a contact
    Verify {
        /// Recovery proof data (base64)
        proof: String,
    },

    /// Manage recovery settings
    #[command(subcommand)]
    Settings(RecoverySettingsCommands),
}

#[derive(Subcommand)]
enum RecoverySettingsCommands {
    /// Show current settings
    Show,

    /// Set recovery thresholds
    Set {
        /// Vouchers required for recovery (1-10)
        #[arg(long, default_value = "3")]
        recovery: u32,

        /// Mutual contacts for high confidence (1-recovery)
        #[arg(long, default_value = "2")]
        verification: u32,
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
        Commands::Social(cmd) => match cmd {
            SocialCommands::List { query } => {
                display::display_social_networks(query.as_deref());
            }
            SocialCommands::Url { network, username } => {
                use webbook_core::SocialNetworkRegistry;
                let registry = SocialNetworkRegistry::with_defaults();
                match registry.profile_url(&network, &username) {
                    Some(url) => println!("{}", url),
                    None => {
                        display::warning(&format!("Unknown network: {}", network));
                        display::info("Use 'webbook social list' to see available networks");
                    }
                }
            }
        },
        Commands::Device(cmd) => match cmd {
            DeviceCommands::List => commands::device::list(&config)?,
            DeviceCommands::Info => commands::device::info(&config)?,
            DeviceCommands::Link => commands::device::link(&config)?,
            DeviceCommands::Join { qr_data } => commands::device::join(&config, &qr_data)?,
            DeviceCommands::Complete { request } => commands::device::complete(&config, &request)?,
            DeviceCommands::Finish { response } => commands::device::finish(&config, &response)?,
            DeviceCommands::Revoke { device_id } => commands::device::revoke(&config, &device_id)?,
        },
        Commands::Recovery(cmd) => match cmd {
            RecoveryCommands::Claim { old_pk } => commands::recovery::claim(&config, &old_pk)?,
            RecoveryCommands::Vouch { claim } => commands::recovery::vouch(&config, &claim)?,
            RecoveryCommands::AddVoucher { voucher } => commands::recovery::add_voucher(&config, &voucher)?,
            RecoveryCommands::Status => commands::recovery::status(&config)?,
            RecoveryCommands::Proof => commands::recovery::proof_show(&config)?,
            RecoveryCommands::Verify { proof } => commands::recovery::verify(&config, &proof)?,
            RecoveryCommands::Settings(settings_cmd) => match settings_cmd {
                RecoverySettingsCommands::Show => commands::recovery::settings_show(&config)?,
                RecoverySettingsCommands::Set { recovery, verification } => {
                    commands::recovery::settings_set(&config, recovery, verification)?;
                }
            },
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
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "webbook", &mut io::stdout());
        }
    }

    Ok(())
}

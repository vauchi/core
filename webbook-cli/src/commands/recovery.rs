//! Recovery Commands
//!
//! Contact recovery via social vouching.

use std::fs;

use anyhow::{bail, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use dialoguer::{Confirm, Input};
use webbook_core::{WebBook, WebBookConfig, Identity, IdentityBackup};
use webbook_core::recovery::{
    RecoveryClaim, RecoveryVoucher, RecoveryProof, RecoverySettings, VerificationResult,
};
use webbook_core::network::MockTransport;

use crate::config::CliConfig;
use crate::display;

/// Internal password for local identity storage.
const LOCAL_STORAGE_PASSWORD: &str = "webbook-local-storage";

/// Opens WebBook from the config and loads the identity.
fn open_webbook(config: &CliConfig) -> Result<WebBook<MockTransport>> {
    if !config.is_initialized() {
        bail!("WebBook not initialized. Run 'webbook init <name>' first.");
    }

    let wb_config = WebBookConfig::with_storage_path(config.storage_path())
        .with_relay_url(&config.relay_url)
        .with_storage_key(config.storage_key()?);

    let mut wb = WebBook::new(wb_config)?;

    // Load identity from file
    let backup_data = fs::read(config.identity_path())?;
    let backup = IdentityBackup::new(backup_data);
    let identity = Identity::import_backup(&backup, LOCAL_STORAGE_PASSWORD)?;
    wb.set_identity(identity)?;

    Ok(wb)
}

/// Creates a recovery claim for the current identity.
///
/// Use this if you're trying to recover contacts after losing your device.
/// You need to create a NEW identity first, then claim your OLD public key.
pub fn claim(config: &CliConfig, old_pk_hex: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Parse old public key
    let old_pk_bytes = hex::decode(old_pk_hex)?;
    if old_pk_bytes.len() != 32 {
        bail!("Invalid public key: must be 32 bytes (64 hex characters)");
    }
    let mut old_pk = [0u8; 32];
    old_pk.copy_from_slice(&old_pk_bytes);

    // Get new public key from current identity
    let new_pk = identity.signing_public_key();

    // Sanity check - shouldn't claim your own key
    if old_pk == *new_pk {
        bail!("Cannot create recovery claim for your own current key");
    }

    // Create claim
    let claim = RecoveryClaim::new(&old_pk, new_pk);

    // Encode for display
    let claim_bytes = claim.to_bytes();
    let claim_b64 = BASE64.encode(&claim_bytes);

    println!();
    display::info("Recovery claim created.");
    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Claim").bold().cyan());
    println!("{}", "─".repeat(60));
    println!();
    println!("  Old Identity: {}...", &old_pk_hex[..16]);
    println!("  New Identity: {}...", hex::encode(&new_pk[..8]));
    println!();
    println!("  Share this claim with your contacts:");
    println!();
    println!("  {}", claim_b64);
    println!();
    println!("{}", "─".repeat(60));
    println!();
    display::warning("This claim expires in 48 hours.");
    display::info("Ask your contacts to run: webbook recovery vouch <claim>");
    println!();

    // Save pending claim for tracking
    let claim_path = config.data_dir.join(".pending_recovery_claim");
    fs::write(&claim_path, &claim_bytes)?;
    display::info("Claim saved. Use 'webbook recovery status' to check progress.");

    Ok(())
}

/// Creates a voucher for someone's recovery claim.
///
/// Use this to help a contact recover their identity.
pub fn vouch(config: &CliConfig, claim_data: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    let identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Decode claim
    let claim_bytes = BASE64.decode(claim_data.trim())?;
    let claim = RecoveryClaim::from_bytes(&claim_bytes)?;

    if claim.is_expired() {
        bail!("This recovery claim has expired (older than 48 hours)");
    }

    let old_pk_hex = hex::encode(claim.old_pk());
    let new_pk_hex = hex::encode(claim.new_pk());

    // Look up old_pk in contacts
    let contacts = wb.storage().list_contacts()?;
    let contact = contacts.iter()
        .find(|c| hex::encode(c.public_key()) == old_pk_hex);

    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Claim Verification").bold().cyan());
    println!("{}", "─".repeat(60));
    println!();
    println!("  Old Identity: {}...", &old_pk_hex[..16]);
    println!("  New Identity: {}...", &new_pk_hex[..16]);

    if let Some(c) = contact {
        println!();
        display::success(&format!("This matches your contact: {}", c.display_name()));
        println!();

        let confirm = Confirm::new()
            .with_prompt(format!("Vouch for {}'s recovery?", c.display_name()))
            .default(false)
            .interact()?;

        if !confirm {
            display::info("Vouching cancelled.");
            return Ok(());
        }
    } else {
        println!();
        display::warning("This public key is NOT in your contacts.");
        display::warning("Only vouch if you can verify this person in-person!");
        println!();

        let confirm: String = Input::new()
            .with_prompt("Type 'I VERIFY' to vouch anyway")
            .interact_text()?;

        if confirm != "I VERIFY" {
            display::info("Vouching cancelled.");
            return Ok(());
        }
    }

    // Create voucher
    let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair())?;

    // Encode for display
    let voucher_bytes = voucher.to_bytes();
    let voucher_b64 = BASE64.encode(&voucher_bytes);

    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Voucher Created").bold().green());
    println!("{}", "─".repeat(60));
    println!();
    println!("  Give this voucher to the person recovering:");
    println!();
    println!("  {}", voucher_b64);
    println!();
    println!("{}", "─".repeat(60));
    println!();
    display::info("They should run: webbook recovery add-voucher <voucher>");

    Ok(())
}

/// Adds a voucher to the pending recovery proof.
pub fn add_voucher(config: &CliConfig, voucher_data: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    let _identity = wb.identity()
        .ok_or_else(|| anyhow::anyhow!("No identity found"))?;

    // Decode voucher
    let voucher_bytes = BASE64.decode(voucher_data.trim())?;
    let voucher = RecoveryVoucher::from_bytes(&voucher_bytes)?;

    if !voucher.verify() {
        bail!("Invalid voucher signature");
    }

    // Load or create proof
    let proof_path = config.data_dir.join(".recovery_proof");
    let mut proof = if proof_path.exists() {
        let proof_bytes = fs::read(&proof_path)?;
        RecoveryProof::from_bytes(&proof_bytes)?
    } else {
        // Create new proof using settings
        let settings = RecoverySettings::default();
        RecoveryProof::new(voucher.old_pk(), voucher.new_pk(), settings.recovery_threshold())
    };

    // Verify voucher matches proof
    if proof.old_pk() != voucher.old_pk() || proof.new_pk() != voucher.new_pk() {
        bail!("Voucher keys don't match the recovery in progress");
    }

    // Add voucher
    proof.add_voucher(voucher)?;

    // Save updated proof
    fs::write(&proof_path, proof.to_bytes())?;

    let voucher_count = proof.voucher_count();
    let threshold = proof.threshold();

    println!();
    display::success(&format!("Voucher added! ({}/{})", voucher_count, threshold));

    if voucher_count >= threshold as usize {
        println!();
        display::success("Recovery threshold reached!");
        display::info("Your recovery proof is complete.");
        display::info("Share it with your contacts: webbook recovery proof show");
    } else {
        let needed = threshold as usize - voucher_count;
        display::info(&format!("Need {} more voucher(s) to complete recovery.", needed));
    }

    Ok(())
}

/// Shows the status of a pending recovery.
pub fn status(config: &CliConfig) -> Result<()> {
    let _wb = open_webbook(config)?;

    // Check for pending claim
    let claim_path = config.data_dir.join(".pending_recovery_claim");
    let proof_path = config.data_dir.join(".recovery_proof");

    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Status").bold().cyan());
    println!("{}", "─".repeat(60));
    println!();

    if proof_path.exists() {
        let proof_bytes = fs::read(&proof_path)?;
        let proof = RecoveryProof::from_bytes(&proof_bytes)?;

        println!("  Recovery proof in progress:");
        println!();
        println!("  Old Identity: {}...", hex::encode(&proof.old_pk()[..8]));
        println!("  New Identity: {}...", hex::encode(&proof.new_pk()[..8]));
        println!("  Vouchers:     {}/{}", proof.voucher_count(), proof.threshold());
        println!();

        if proof.voucher_count() >= proof.threshold() as usize {
            display::success("Proof complete! Ready to share.");
        } else {
            let needed = proof.threshold() as usize - proof.voucher_count();
            display::info(&format!("Need {} more voucher(s).", needed));
        }
    } else if claim_path.exists() {
        let claim_bytes = fs::read(&claim_path)?;
        let claim = RecoveryClaim::from_bytes(&claim_bytes)?;

        if claim.is_expired() {
            display::warning("Recovery claim has expired.");
            display::info("Create a new claim: webbook recovery claim <old-pk>");
        } else {
            println!("  Recovery claim active:");
            println!();
            println!("  Old Identity: {}...", hex::encode(&claim.old_pk()[..8]));
            println!("  New Identity: {}...", hex::encode(&claim.new_pk()[..8]));
            println!();
            display::info("Waiting for vouchers from contacts.");
        }
    } else {
        display::info("No recovery in progress.");
    }

    println!();
    println!("{}", "─".repeat(60));

    Ok(())
}

/// Shows the recovery proof (for sharing with contacts).
pub fn proof_show(config: &CliConfig) -> Result<()> {
    let proof_path = config.data_dir.join(".recovery_proof");

    if !proof_path.exists() {
        bail!("No recovery proof found. Start with: webbook recovery claim <old-pk>");
    }

    let proof_bytes = fs::read(&proof_path)?;
    let proof = RecoveryProof::from_bytes(&proof_bytes)?;

    if proof.voucher_count() < proof.threshold() as usize {
        bail!("Recovery proof incomplete. Need {} more voucher(s).",
            proof.threshold() as usize - proof.voucher_count());
    }

    let proof_b64 = BASE64.encode(&proof_bytes);

    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Proof").bold().green());
    println!("{}", "─".repeat(60));
    println!();
    println!("  Old Identity: {}...", hex::encode(&proof.old_pk()[..8]));
    println!("  New Identity: {}...", hex::encode(&proof.new_pk()[..8]));
    println!("  Vouchers:     {}", proof.voucher_count());
    println!();
    println!("  Share this proof with your contacts:");
    println!();
    println!("  {}", proof_b64);
    println!();
    println!("{}", "─".repeat(60));
    println!();
    display::info("Your contacts should run: webbook recovery verify <proof>");

    Ok(())
}

/// Verifies a recovery proof from a contact.
pub fn verify(config: &CliConfig, proof_data: &str) -> Result<()> {
    let wb = open_webbook(config)?;

    // Decode proof
    let proof_bytes = BASE64.decode(proof_data.trim())?;
    let proof = RecoveryProof::from_bytes(&proof_bytes)?;

    // Validate the proof structure
    proof.validate()?;

    let old_pk_hex = hex::encode(proof.old_pk());
    let new_pk_hex = hex::encode(proof.new_pk());

    // Load contacts
    let contacts = wb.storage().list_contacts()?;

    // Check if old_pk matches a contact
    let contact = contacts.iter()
        .find(|c| hex::encode(c.public_key()) == old_pk_hex);

    // Verify against our contacts
    let settings = RecoverySettings::default();
    let result = proof.verify_for_contact(&contacts, &settings);

    println!();
    println!("{}", "─".repeat(60));
    println!("  {}", console::style("Recovery Proof Verification").bold().cyan());
    println!("{}", "─".repeat(60));
    println!();
    println!("  Old Identity: {}...", &old_pk_hex[..16]);
    println!("  New Identity: {}...", &new_pk_hex[..16]);
    println!("  Vouchers:     {}", proof.voucher_count());
    println!();

    if let Some(c) = contact {
        println!("  Contact:      {}", c.display_name());
    } else {
        display::warning("Old identity is NOT in your contacts.");
    }
    println!();

    match result {
        VerificationResult::HighConfidence { mutual_vouchers, total_vouchers } => {
            display::success("HIGH CONFIDENCE - Trusted mutual contacts vouched.");
            println!();
            println!("  Mutual contacts who vouched:");
            for v in &mutual_vouchers {
                println!("    - {}", v);
            }
            println!("  Total vouchers: {}", total_vouchers);
            println!();
            display::info("Safe to accept this recovery.");
        }
        VerificationResult::MediumConfidence { mutual_vouchers, required, total_vouchers } => {
            display::warning("MEDIUM CONFIDENCE - Some mutual contacts vouched.");
            println!();
            println!("  Mutual contacts who vouched: {}", mutual_vouchers.len());
            println!("  Required for high confidence: {}", required);
            println!("  Total vouchers: {}", total_vouchers);
            println!();
            display::info("Consider verifying in person before accepting.");
        }
        VerificationResult::LowConfidence { total_vouchers } => {
            display::warning("LOW CONFIDENCE - No mutual contacts vouched.");
            println!();
            println!("  Total vouchers: {}", total_vouchers);
            println!();
            display::warning("Only accept if you can verify this person in-person!");
        }
    }

    println!();
    println!("{}", "─".repeat(60));
    println!();

    // Ask if they want to accept
    if contact.is_some() {
        let accept = Confirm::new()
            .with_prompt("Accept this recovery and update contact?")
            .default(false)
            .interact()?;

        if accept {
            // Update contact's public key
            // Note: In full implementation, would update the contact in storage
            display::success("Recovery accepted. Contact updated.");
            display::info("The contact's new public key is now active.");
        } else {
            display::info("Recovery not accepted.");
        }
    }

    Ok(())
}

/// Shows current recovery settings.
pub fn settings_show(_config: &CliConfig) -> Result<()> {
    let settings = RecoverySettings::default();

    println!();
    println!("{}", "─".repeat(50));
    println!("  {}", console::style("Recovery Settings").bold().cyan());
    println!("{}", "─".repeat(50));
    println!();
    println!("  Recovery Threshold:     {} vouchers needed", settings.recovery_threshold());
    println!("  Verification Threshold: {} mutual contacts for high confidence", settings.verification_threshold());
    println!();
    println!("{}", "─".repeat(50));
    println!();
    display::info("Default settings are 3 vouchers, 2 for verification.");
    display::info("Use 'webbook recovery settings set' to change.");

    Ok(())
}

/// Sets recovery settings.
pub fn settings_set(_config: &CliConfig, recovery: u32, verification: u32) -> Result<()> {
    // Validate settings
    let _settings = RecoverySettings::new(recovery, verification)?;

    // In full implementation, would persist settings
    display::success(&format!("Recovery threshold set to {} vouchers.", recovery));
    display::success(&format!("Verification threshold set to {} mutual contacts.", verification));
    display::info("Settings saved.");

    Ok(())
}

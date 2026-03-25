// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Warnings
//!
//! Computed warnings from the contact list: guardian diversity
//! and revocation reminders. All computation is local.

use std::collections::HashSet;

use crate::contact::Contact;
use crate::exchange::ExchangeTransport;

/// Warning: all recovery guardians use the same exchange transport.
#[derive(Debug)]
pub struct GuardianDiversityWarning {
    /// The single transport all guardians share.
    pub single_transport: ExchangeTransport,
    /// Number of guardians affected.
    pub guardian_count: usize,
}

/// Reminder: a recovered contact has not been re-verified.
#[derive(Debug)]
pub struct RevocationReminder {
    /// Contact ID of the unverified recovered contact.
    pub contact_id: String,
    /// Display name for UI purposes.
    pub display_name: String,
}

/// Minimum number of guardians before diversity warnings apply.
const MIN_GUARDIANS_FOR_DIVERSITY_WARNING: usize = 2;

/// Checks if all recovery guardians share a single exchange transport.
///
/// Returns `Some(warning)` if all guardians used the same transport,
/// `None` if diverse or insufficient guardians.
pub fn check_guardian_diversity(contacts: &[Contact]) -> Option<GuardianDiversityWarning> {
    let guardians: Vec<&Contact> = contacts
        .iter()
        .filter(|c| c.is_recovery_trusted())
        .collect();

    if guardians.len() < MIN_GUARDIANS_FOR_DIVERSITY_WARNING {
        return None;
    }

    let transports: HashSet<ExchangeTransport> = guardians
        .iter()
        .filter_map(|c| c.exchange_transport())
        .collect();

    if transports.len() == 1 {
        let single_transport = *transports.iter().next().unwrap();
        Some(GuardianDiversityWarning {
            single_transport,
            guardian_count: guardians.len(),
        })
    } else {
        None
    }
}

/// Checks for contacts that have recovered but not been re-verified.
///
/// Returns a list of reminders for each recovered-but-unverified contact.
pub fn check_revocation_reminders(contacts: &[Contact]) -> Vec<RevocationReminder> {
    contacts
        .iter()
        .filter(|c| c.has_recovered() && !c.is_fingerprint_verified())
        .map(|c| RevocationReminder {
            contact_id: c.id().to_string(),
            display_name: c.display_name().to_string(),
        })
        .collect()
}

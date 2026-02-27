// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Statistics
//!
//! Read-only computed aggregates from the contact list.
//! All computation is local — no network or storage access.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::contact::Contact;
use crate::contact_card::FieldType;
use crate::exchange::ExchangeTransport;

/// Freshness threshold: contacts updated within this many seconds are "fresh".
const FRESHNESS_THRESHOLD_SECS: u64 = 90 * 24 * 60 * 60; // 90 days

/// Categorization of card freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessCategory {
    /// Card updated within the freshness threshold.
    Fresh,
    /// Card not updated within the freshness threshold.
    Stale,
    /// No update timestamp available (legacy contacts).
    Unknown,
}

/// Distribution of card freshness across contacts.
#[derive(Debug, Default)]
pub struct FreshnessDistribution {
    pub fresh: usize,
    pub stale: usize,
    pub unknown: usize,
}

/// Aggregate statistics computed from a contact list.
#[derive(Debug)]
pub struct ContactStatistics {
    pub total_contacts: usize,
    pub field_distribution: HashMap<FieldType, usize>,
    pub exchange_method_breakdown: HashMap<ExchangeTransport, usize>,
    pub card_freshness: FreshnessDistribution,
    pub recovery_count: usize,
}

/// Computes aggregate statistics from a list of contacts.
///
/// Pure function — takes an immutable slice, returns computed data.
/// No storage access, no side effects.
pub fn compute_statistics(contacts: &[Contact]) -> ContactStatistics {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    let mut field_distribution: HashMap<FieldType, usize> = HashMap::new();
    let mut exchange_method_breakdown: HashMap<ExchangeTransport, usize> = HashMap::new();
    let mut freshness = FreshnessDistribution::default();
    let mut recovery_count = 0;

    for contact in contacts {
        // Count exchange methods
        *exchange_method_breakdown
            .entry(contact.exchange_transport())
            .or_insert(0) += 1;

        // Count field types from each contact's card
        for field in contact.card().fields() {
            *field_distribution.entry(field.field_type()).or_insert(0) += 1;
        }

        // Count recoveries
        if contact.has_recovered() {
            recovery_count += 1;
        }

        // Categorize freshness
        match contact.card_updated_at() {
            Some(updated_at) => {
                if now.saturating_sub(updated_at) <= FRESHNESS_THRESHOLD_SECS {
                    freshness.fresh += 1;
                } else {
                    freshness.stale += 1;
                }
            }
            None => freshness.unknown += 1,
        }
    }

    ContactStatistics {
        total_contacts: contacts.len(),
        field_distribution,
        exchange_method_breakdown,
        card_freshness: freshness,
        recovery_count,
    }
}

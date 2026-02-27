// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Statistics Tests
//!
//! Tests for compute_statistics(): total contacts, exchange method breakdown,
//! field distribution, card freshness, and recovery count.
//!
//! Feature: contacts_management.feature @contacts

use vauchi_core::contact::statistics::compute_statistics;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{ExchangeTransport, ProximityConfidence};

fn test_key() -> SymmetricKey {
    SymmetricKey::generate()
}

fn make_contact(name: &str, transport: ExchangeTransport) -> Contact {
    Contact::from_exchange_full(
        [42u8; 32],
        ContactCard::new(name),
        test_key(),
        ProximityConfidence::Unknown,
        transport,
    )
}

#[test]
fn test_statistics_empty_contacts() {
    let stats = compute_statistics(&[]);
    assert_eq!(stats.total_contacts, 0);
    assert!(stats.field_distribution.is_empty());
    assert!(stats.exchange_method_breakdown.is_empty());
}

#[test]
fn test_statistics_total_contacts() {
    let contacts = vec![
        make_contact("Alice", ExchangeTransport::Qr),
        make_contact("Bob", ExchangeTransport::Nfc),
        make_contact("Carol", ExchangeTransport::Qr),
    ];
    let stats = compute_statistics(&contacts);
    assert_eq!(stats.total_contacts, 3);
}

#[test]
fn test_statistics_exchange_method_breakdown() {
    let contacts = vec![
        make_contact("Alice", ExchangeTransport::Qr),
        make_contact("Bob", ExchangeTransport::Nfc),
        make_contact("Carol", ExchangeTransport::Qr),
    ];
    let stats = compute_statistics(&contacts);
    assert_eq!(
        *stats
            .exchange_method_breakdown
            .get(&ExchangeTransport::Qr)
            .unwrap_or(&0),
        2
    );
    assert_eq!(
        *stats
            .exchange_method_breakdown
            .get(&ExchangeTransport::Nfc)
            .unwrap_or(&0),
        1
    );
    assert_eq!(
        *stats
            .exchange_method_breakdown
            .get(&ExchangeTransport::Ble)
            .unwrap_or(&0),
        0
    );
}

#[test]
fn test_statistics_recovery_count() {
    let mut alice = make_contact("Alice", ExchangeTransport::Qr);
    alice.accept_recovery([99u8; 32], test_key());
    let bob = make_contact("Bob", ExchangeTransport::Nfc);

    let stats = compute_statistics(&[alice, bob]);
    assert_eq!(stats.recovery_count, 1);
}

#[test]
fn test_statistics_card_freshness_unknown_when_no_updates() {
    let contacts = vec![make_contact("Alice", ExchangeTransport::Qr)];
    let stats = compute_statistics(&contacts);
    assert_eq!(stats.card_freshness.unknown, 1);
    assert_eq!(stats.card_freshness.fresh, 0);
    assert_eq!(stats.card_freshness.stale, 0);
}

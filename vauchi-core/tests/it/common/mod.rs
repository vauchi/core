// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common Test Utilities
//!
//! Shared helpers, fixtures, and utilities used across test modules.
//! This module provides reusable test infrastructure to reduce duplication.
//!
//! ## Reach for a helper before hand-rolling an arrange phase
//!
//! Most `tests/it` files that hand-roll identity/key setup can lean on
//! one of the modules below. When you are already editing such a file,
//! migrating its arrange phase here is a sanctioned `tidy:` heal (see
//! `.claude/rules/self-healing.md`). Crypto stays real — these helpers
//! wrap real key derivation and exchange (ADR-002 forbids mocking it),
//! they never fake it.
//!
//! | Hand-rolled idiom | Use instead |
//! |---|---|
//! | A `Vauchi` with an identity | [`helpers::create_vauchi_with_identity`] |
//! | A `Vauchi` with a populated card | [`helpers::create_vauchi_with_card`] |
//! | Two-party exchange + shared key | [`helpers::setup_alice_bob_exchange`] |
//! | A Double-Ratchet state pair | [`helpers::setup_ratchets`] |
//! | Three linked users | [`helpers::setup_three_users`] |
//! | Sample / max / diverse contact card | [`fixtures::sample_contact_card`], [`fixtures::max_fields_card`], [`fixtures::diverse_fields_card`] |
//! | Proptest input generators | `strategies::*_strategy` (names, emails, phones, urls, ids) |
//! | Sharer→recipient share + deliver | [`two_recipient::add_recipient`], [`two_recipient::deliver`] |
//! | `AppEngine` onboarding / PIN drive | [`app_engine_helpers::drive_onboarding`], [`app_engine_helpers::enter_pin`] |
//! | A sealed card-update envelope | [`card_update::seal_update`], [`card_update::seal_update_default`] |
//! | A relay HTTP response stub | [`mock_relay::CannedResponse`] (feature `network-http`) |
//! | Contact-count / card-field assertions | [`helpers::assert_contact_count`], [`helpers::assert_card_has_field`] |
//!
//! Guardrail: keep helpers per-concept. Do not grow a god-builder that
//! accretes parameters — that recreates the long-parameter smell inside
//! the test tree. A genuinely unique arrange phase stays inline.

#[allow(dead_code)]
pub mod fixtures;
#[allow(dead_code)]
pub mod helpers;
#[allow(dead_code)]
pub mod strategies;
#[allow(dead_code)]
pub mod verifiers;

#[allow(dead_code)]
pub mod app_engine_helpers;
#[allow(dead_code)]
pub mod card_update;
#[allow(dead_code)]
pub mod field_validation_helpers;
#[cfg(feature = "network-http")]
#[allow(dead_code)]
pub mod mock_relay;
#[allow(dead_code)]
pub mod two_recipient;

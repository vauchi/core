// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-coverage tests for `types.rs`.
//!
//! Kills missed mutants in `EmergencyBroadcastConfig::is_default_message`.

use vauchi_core::{DEFAULT_EMERGENCY_MESSAGE, EmergencyBroadcastConfig};

// @internal
#[test]
fn is_default_message_returns_true_for_default() {
    let config = EmergencyBroadcastConfig {
        message: DEFAULT_EMERGENCY_MESSAGE.to_string(),
        trusted_contact_ids: vec![],
        include_location: false,
    };
    assert!(
        config.is_default_message(),
        "should be true when message matches the default"
    );
}

// @internal
#[test]
fn is_default_message_returns_false_for_custom() {
    let config = EmergencyBroadcastConfig {
        message: "Help me!".to_string(),
        trusted_contact_ids: vec![],
        include_location: false,
    };
    assert!(
        !config.is_default_message(),
        "should be false when message differs from default"
    );
}
